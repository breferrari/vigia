//! `criterion` benchmarks for the engine, named by `SPEC.md` §7.
//!
//! These track and report; they do not gate. The gates live in
//! `tests/budgets.rs`, because criterion measures change against a baseline
//! while the budgets in `SPEC.md` are absolute. Both are wanted: criterion says
//! "this got 20% slower", the budget test says "this is now over the line".
//!
//! The fixture is shared with the test suite rather than duplicated, so a
//! benchmark and a gate can never drift into measuring different things.

#[path = "../tests/support/mod.rs"]
mod support;

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use vigia_core::{ChangeOptions, Worktree};

/// Matches `tests/budgets.rs`: 100 x 500 rewritten is a 100k-line diff.
const FILES: usize = 100;
const LINES: usize = 500;

fn engine(c: &mut Criterion) {
    let scratch = support::Scratch::large_diff("bench-engine", FILES, LINES);
    let worktree: Worktree = scratch.worktree();
    let changes: Vec<_> = worktree
        .changes()
        .expect("enumerate")
        .map(|change| change.expect("change"))
        .collect();
    assert_eq!(
        changes.len(),
        FILES,
        "fixture did not produce {FILES} changes"
    );

    let mut group = c.benchmark_group("engine");

    // The floor every frame pays, incremental or not.
    group.bench_function("enumerate", |b| {
        b.iter(|| black_box(worktree.changes().expect("enumerate").count()))
    });

    // Rename tracking is the option that changes the shape of the stream, so
    // its cost is tracked separately rather than folded into the default.
    group.bench_function("enumerate_without_renames", |b| {
        b.iter(|| {
            black_box(
                worktree
                    .changes_with(ChangeOptions {
                        track_renames: false,
                    })
                    .expect("enumerate")
                    .count(),
            )
        })
    });

    // What a frame costs when one file changed.
    group.bench_function("diff_one_file", |b| {
        b.iter(|| black_box(worktree.diff(&changes[0]).expect("diff")))
    });

    // What a frame would cost without incrementality. The gap between this and
    // the line above is the whole of I2a's justification.
    group.bench_function("diff_every_file", |b| {
        b.iter(|| {
            for change in &changes {
                black_box(worktree.diff(change).expect("diff"));
            }
        })
    });

    // I7 and I4 together: open cold, then paint the first file.
    let root = scratch.path_of(".");
    group.bench_function("open_and_first_paint", |b| {
        b.iter(|| {
            let worktree = Worktree::discover(&root).expect("discover");
            let change = worktree
                .changes()
                .expect("enumerate")
                .next()
                .expect("a change")
                .expect("change");
            black_box(worktree.diff(&change).expect("diff"))
        })
    });

    group.finish();
}

criterion_group!(benches, engine);
criterion_main!(benches);
