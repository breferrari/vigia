//! Phase 1 harness: no TUI, just frame timings.
//!
//! `SPEC.md` §8 asks Phase 1 for "`vigia-core` plus a `main` that prints frame
//! timings", because the open question that gates everything else is whether
//! `gix` gives working-tree-vs-index diffs at the fidelity and speed needed.
//! This binary is how that question gets answered against a real repository
//! instead of a fixture.
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
//!   once incremental re-diffing (I2) exists, so it is reported as the cost
//!   that incrementality has to remove, not as a frame time.

use std::process::ExitCode;
use std::time::{Duration, Instant};

use vigia_core::{ChangeOptions, Error, Samples, Worktree};

/// Sweeps taken after the cold one, for a steady-state percentile.
const WARM_SWEEPS: usize = 20;

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

    println!("steady state  (warm, n={WARM_SWEEPS})");
    if let (Some(p50), Some(p99)) = (incremental.percentile(0.50), incremental.percentile(0.99)) {
        println!(
            "  I9  re-diff one file    p99 {:>9}  budget {:>8}  {}   (p50 {})",
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
            "cost I2 removes",
            ms(p50)
        );
    }
    println!(
        "      of which enumerate      {:>9}   paid by every frame regardless",
        ms(full.enumerate)
    );

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
///
/// The limit is what separates a first-paint measurement from a whole-diff
/// one. Enumeration always runs to completion because a monitor has to know
/// the full file list to draw a scrollbar, even when it only renders the top.
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
