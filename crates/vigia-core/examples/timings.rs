//! Frame timings against a real repository: no TUI, just numbers.
//!
//! ```text
//! cargo run --release -p vigia-core --example timings -- <path>
//! ```
//!
//! `SPEC.md` §8 asks Phase 1 for "`vigia-core` plus a `main` that prints frame
//! timings", because the open question that gates everything else is whether
//! `gix` gives working-tree-vs-index diffs at the fidelity and speed needed.
//! This is how that question gets answered against a real repository instead of
//! a fixture. It was the `vigia` binary until Phase 2 gave that binary a
//! terminal to draw in; it moved here rather than being deleted, because three
//! of the open questions in `SPEC.md` §10 are still measured with it.
//!
//! It reports, and does not gate. The budgets in `SPEC.md` are defined against
//! named fixtures, so the CI gate belongs in the test and benchmark suites; a
//! number measured against whatever repository you happen to point at is
//! evidence, not a verdict.
//!
//! Two of the measurements below are deliberately separated, because
//! conflating them is the easy way to report a budget breach that is not one:
//!
//! * **first paint** diffs only what a screen could show. That is what I7 and
//!   I4 are about.
//! * **full sweep** diffs every changed file. No frame ever has to do this
//!   once incremental re-diffing (I2a) exists, so it is reported as the cost
//!   that incrementality has to remove, not as a frame time.
//!
//! It reads and never writes. Every number here is measured against the
//! repository as it stands, which is why the frame section reports a
//! *revalidating* frame rather than one following a simulated edit: fabricating
//! an edit would mean writing to someone's working tree to benchmark it.

use std::process::ExitCode;
use std::time::{Duration, Instant};

use vigia_core::{ChangeOptions, Error, Frame, FrameStats, Samples, Worktree};

/// Sweeps taken after the cold one, for a steady-state percentile.
const WARM_SWEEPS: usize = 20;

/// Frames allowed for a working tree's recent writes to settle.
const SETTLE_FRAMES: usize = 8;

/// Files a first paint is assumed to need. A tall terminal shows fewer.
const FIRST_PAINT_FILES: usize = 1;

/// I7: startup to first paint.
const BUDGET_STARTUP: Duration = Duration::from_millis(50);
/// I4: first paint on a 100k-line diff.
const BUDGET_FIRST_PAINT: Duration = Duration::from_millis(100);
/// I9: steady-state frame time under continuous edits.
const BUDGET_FRAME: Duration = Duration::from_millis(16);

fn main() -> ExitCode {
    let started = Instant::now();

    let path = std::env::args_os()
        .nth(1)
        .unwrap_or_else(|| std::ffi::OsString::from("."));

    let worktree = match Worktree::discover(&path) {
        Ok(worktree) => worktree,
        Err(e) => {
            eprintln!("vigia: {e}");
            return ExitCode::FAILURE;
        }
    };

    let renames_on = ChangeOptions {
        track_renames: true,
    };
    let renames_off = ChangeOptions {
        track_renames: false,
    };

    // Cold, and measured from process start: this is the number I7 is about.
    let cold = match sweep(&worktree, renames_on, FIRST_PAINT_FILES) {
        Ok(sweep) => sweep,
        Err(e) => {
            eprintln!("vigia: {e}");
            return ExitCode::FAILURE;
        }
    };
    let to_first_paint = started.elapsed();

    let full = match sweep(&worktree, renames_on, usize::MAX) {
        Ok(sweep) => sweep,
        Err(e) => {
            eprintln!("vigia: {e}");
            return ExitCode::FAILURE;
        }
    };

    println!("vigia phase 1 harness");
    println!("  worktree  {}", worktree.workdir().display());
    println!(
        "  changes   {} files, +{} -{}, {} of content",
        full.files,
        full.added,
        full.removed,
        bytes(full.bytes)
    );
    println!();

    println!("first paint  (enumerate, then diff {FIRST_PAINT_FILES} file)");
    println!(
        "  I7  process start to painted  {:>9}  budget {:>8}  {}",
        ms(to_first_paint),
        ms(BUDGET_STARTUP),
        verdict(to_first_paint, BUDGET_STARTUP)
    );
    println!(
        "  I4  first change available    {:>9}  budget {:>8}  {}",
        ms(cold.first_change),
        ms(BUDGET_FIRST_PAINT),
        verdict(cold.first_change, BUDGET_FIRST_PAINT)
    );
    println!();

    let mut incremental = Samples::new(WARM_SWEEPS);
    let mut naive = Samples::new(WARM_SWEEPS);
    for _ in 0..WARM_SWEEPS {
        match (
            sweep(&worktree, renames_on, FIRST_PAINT_FILES),
            sweep(&worktree, renames_on, usize::MAX),
        ) {
            (Ok(one), Ok(all)) => {
                incremental.push(one.elapsed);
                naive.push(all.elapsed);
            }
            (Err(e), _) | (_, Err(e)) => {
                eprintln!("vigia: {e}");
                return ExitCode::FAILURE;
            }
        }
    }

    // Deliberately not labelled I9. These are the raw primitives with no memory
    // of the previous frame, which is the floor a frame cannot beat rather than
    // any frame a monitor has. The frame path is measured below, and it is what
    // `tests/budgets.rs` gates I9 over.
    println!("primitives, the floor  (warm, n={WARM_SWEEPS})");
    if let (Some(p50), Some(p99)) = (incremental.percentile(0.50), incremental.percentile(0.99)) {
        println!(
            "      re-diff one file    p99 {:>9}  budget {:>8}  {}   (p50 {})",
            ms(p99),
            ms(BUDGET_FRAME),
            verdict(p99, BUDGET_FRAME),
            ms(p50)
        );
    }
    if let (Some(p50), Some(p99)) = (naive.percentile(0.50), naive.percentile(0.99)) {
        println!(
            "      re-diff everything  p99 {:>9}  {:>17}   (p50 {})",
            ms(p99),
            "cost I2a removes",
            ms(p50)
        );
    }
    println!(
        "      of which enumerate      {:>9}   paid by every frame regardless",
        ms(full.enumerate)
    );

    // I2a on a real repository. The two lines below run the same work through
    // the same type; the only difference is whether the previous frame's
    // answers were available. Their ratio is what incrementality bought.
    match frame_costs(&worktree) {
        Ok(frames) => {
            println!();
            println!("frame path  (Frame, every file materialised, n={WARM_SWEEPS})");
            println!(
                "      no edit is simulated, so the warm line is a revalidating \
                 frame, not I9's"
            );
            if let (Some(p50), Some(p99)) =
                (frames.cold.percentile(0.50), frames.cold.percentile(0.99))
            {
                println!(
                    "      cold, no frame to reuse   p99 {:>9}   {:>10} read   (p50 {})",
                    ms(p99),
                    bytes(frames.cold_stats.bytes),
                    ms(p50)
                );
            }
            if let (Some(p50), Some(p99)) =
                (frames.warm.percentile(0.50), frames.warm.percentile(0.99))
            {
                println!(
                    "  I2a revalidating a frame      p99 {:>9}   {:>10} read   (p50 {})",
                    ms(p99),
                    bytes(frames.warm_stats.bytes),
                    ms(p50)
                );
            }
            println!(
                "      {} files, {} diffs held, {} recomputed, {} reused, {} stats",
                frames.files,
                frames.tracked,
                frames.warm_stats.computed,
                frames.warm_stats.reused,
                frames.warm_stats.probes
            );
            if frames.warm_stats.computed > 0 {
                println!(
                    "      note: {} file(s) were written too recently to prove unchanged, \
                     so they were re-read",
                    frames.warm_stats.computed
                );
            }
        }
        Err(e) => {
            eprintln!("vigia: {e}");
            return ExitCode::FAILURE;
        }
    }

    // Rename tracking is the one option that changes the *shape* of the
    // stream rather than only its cost, so the harness reports both.
    println!();
    println!("does it stream?  (time to first change as a share of the whole walk)");
    for (label, options) in [("renames on ", renames_on), ("renames off", renames_off)] {
        let mut first = Samples::new(WARM_SWEEPS);
        let mut whole = Samples::new(WARM_SWEEPS);
        for _ in 0..WARM_SWEEPS {
            match sweep(&worktree, options, 0) {
                Ok(sweep) => {
                    first.push(sweep.first_change);
                    whole.push(sweep.enumerate);
                }
                Err(e) => {
                    eprintln!("vigia: {e}");
                    return ExitCode::FAILURE;
                }
            }
        }
        if let (Some(f), Some(w)) = (first.percentile(0.50), whole.percentile(0.50)) {
            let share = f.as_secs_f64() / w.as_secs_f64().max(f64::EPSILON) * 100.0;
            println!(
                "  {label}  first {:>8}  whole walk {:>8}  {share:>3.0}%",
                ms(f),
                ms(w)
            );
        }
    }

    println!();
    println!("Numbers are evidence against this repository, not a CI verdict.");

    ExitCode::SUCCESS
}

/// What the frame path cost with and without a previous frame to lean on.
struct FrameCosts {
    /// A fresh [`Frame`] each time, so nothing can be reused.
    cold: Samples,
    /// A settled frame, revalidating what it already holds.
    warm: Samples,
    /// Counters for one cold frame, and for one warm frame.
    cold_stats: FrameStats,
    warm_stats: FrameStats,
    files: usize,
    tracked: usize,
}

/// Advance one frame and fetch every diff in it.
fn materialise(frame: &mut Frame) -> Result<(), Error> {
    frame.advance()?;
    for i in 0..frame.files().len() {
        frame.diff(i)?;
    }
    Ok(())
}

/// What one frame cost, as the difference between two cumulative readings.
fn delta(before: FrameStats, after: FrameStats) -> FrameStats {
    FrameStats {
        computed: after.computed - before.computed,
        reused: after.reused - before.reused,
        measured: after.measured - before.measured,
        bytes: after.bytes - before.bytes,
        probes: after.probes - before.probes,
        evicted: after.evicted - before.evicted,
    }
}

/// Measure I2a against this repository: the same frame, with and without memory.
fn frame_costs(worktree: &Worktree) -> Result<FrameCosts, Error> {
    let mut cold = Samples::new(WARM_SWEEPS);
    let mut warm = Samples::new(WARM_SWEEPS);

    let mut cold_stats = FrameStats::default();
    for _ in 0..WARM_SWEEPS {
        let mut frame = worktree.frame();
        let start = Instant::now();
        materialise(&mut frame)?;
        cold.push(start.elapsed());
        cold_stats = frame.stats();
    }

    let mut frame = worktree.frame();
    for _ in 0..SETTLE_FRAMES {
        let before = frame.stats().computed;
        materialise(&mut frame)?;
        if frame.stats().computed == before {
            break;
        }
    }

    let mut warm_stats = FrameStats::default();
    for _ in 0..WARM_SWEEPS {
        let before = frame.stats();
        let start = Instant::now();
        materialise(&mut frame)?;
        warm.push(start.elapsed());
        warm_stats = delta(before, frame.stats());
    }

    Ok(FrameCosts {
        cold,
        warm,
        cold_stats,
        warm_stats,
        files: frame.files().len(),
        tracked: frame.tracked(),
    })
}

/// What one measured sweep produced.
struct Sweep {
    /// Wall time for the whole sweep.
    elapsed: Duration,
    /// Time until the first change came out of the stream.
    first_change: Duration,
    /// Time to walk the tree and name every change.
    enumerate: Duration,
    /// Changed files found.
    files: usize,
    added: u32,
    removed: u32,
    bytes: u64,
}

/// Enumerate every change, then diff at most `diff_limit` of them.
fn sweep(worktree: &Worktree, options: ChangeOptions, diff_limit: usize) -> Result<Sweep, Error> {
    let start = Instant::now();

    let mut first_change = None;
    let mut changes = Vec::new();
    for change in worktree.changes_with(options)? {
        if first_change.is_none() {
            first_change = Some(start.elapsed());
        }
        changes.push(change?);
    }
    let enumerate = start.elapsed();

    let mut sweep = Sweep {
        elapsed: Duration::ZERO,
        // A clean worktree yields nothing, so first paint is the empty frame.
        first_change: first_change.unwrap_or(enumerate),
        enumerate,
        files: changes.len(),
        added: 0,
        removed: 0,
        bytes: 0,
    };

    for change in changes.iter().take(diff_limit) {
        let diff = worktree.diff(change)?;
        sweep.added += diff.added;
        sweep.removed += diff.removed;
        sweep.bytes += diff.bytes;
    }

    sweep.elapsed = start.elapsed();
    Ok(sweep)
}

fn ms(d: Duration) -> String {
    format!("{:.2}ms", d.as_secs_f64() * 1000.0)
}

fn bytes(n: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    let n = n as f64;
    if n >= MIB {
        format!("{:.1} MiB", n / MIB)
    } else if n >= KIB {
        format!("{:.1} KiB", n / KIB)
    } else {
        format!("{n:.0} B")
    }
}

fn verdict(actual: Duration, budget: Duration) -> &'static str {
    if actual <= budget { "within" } else { "OVER" }
}
