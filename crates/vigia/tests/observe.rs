//! [#72](https://github.com/breferrari/vigia/issues/72): the workload, measured.
//!
//! Every other number in this repository comes from a fixture.
//! `crates/vigia-core/tests/budgets.rs` builds 100 files of 500 lines and
//! rewrites them line for line, the soak drives the same shape, and
//! `crates/vigia-core/examples/timings.rs` drives the core. Each is a good
//! instrument and none of them is a repository with an agent writing to it,
//! which is the one condition `SPEC.md` §2 buys the product class on.
//!
//! ## What this is not
//!
//! **It is not a gate.** Nothing here asserts anything about the product: the
//! window is whatever a working session happened to be and the workload is
//! whatever an agent happened to do, so a threshold over either would be a
//! budget `SPEC.md` does not name. It reports, and the reports go into §10
//! beside the fixture numbers they exist to be compared against. A resource
//! claim is *gated* by the soak, over the soak's window, and that stays true.
//!
//! **It is not a product surface.** §11.2 B7 declined `VIGIA_OBSERVE` on
//! 2026-08-05, and point 5 of that decline is why this file exists rather than a
//! flag: *"an instrument for measuring this product already has a home, and it
//! is not the product."* Nothing here is reachable from the binary, and the
//! monitor still writes nothing.
//!
//! ## The three instruments, and why they are three processes
//!
//! 1. **The dirty-file sampler is not in this file at all.** How many files are
//!    dirty at once is a property of the **worktree**, so anything that can read
//!    the worktree measures it: `git status --porcelain`, counted, on a timer,
//!    in a pane. That is B7 point 2 verbatim, and building it into a Rust
//!    harness would be the product-code answer that decline already rejected.
//!
//! 2. [`observe_a_working_session`] is the measured loop: `vigia::run` with the
//!    terminal taken out, pointed at an existing worktree instead of a fixture,
//!    with **no writer of its own**. The agent in the other pane is the
//!    workload, and injecting synthetic edits or scripted keys would make this a
//!    second soak rather than a measurement of use.
//!
//! 3. [`observe_the_diff_against_git`] is the oracle, and it runs in a **process
//!    of its own** rather than on a timer inside 2. That is not tidiness.
//!    Answering *"does this diff agree with git"* means diffing every changed
//!    file, and §7 already records what happens when a measurement's own setup
//!    materialises every unit: `settle()` populated the cache the gate was
//!    about, eleven read-bounding gates stayed green, and the height walk was
//!    re-reading the whole undrawn changed set at 18.36ms p99 against a 16ms
//!    budget. An oracle sharing a [`vigia_core::Frame`] with the measured loop
//!    would warm exactly the caches whose cost is the measurement.
//!
//! ## Reading it
//!
//! Both entry points are `#[ignore]`, like `soak_child`: they are run
//! deliberately, against a tree the environment names, and they print rather
//! than assert. What `cargo test` does carry is [`statistic`], which holds every
//! pure function either one draws a conclusion from, because a number nobody can
//! check is worse than no number at all.

/// The fixture builder, reached the way `soak.rs` reaches it.
///
/// `SPEC.md` §9 names this escape: a published `.crate` does not carry the
/// sibling crate's test tree, so `cargo test` inside an unpacked copy fails
/// where it passes in a checkout, and the fix at publish time is to exclude the
/// tests from the package rather than to weaken a gate. This is the second file
/// to make it and §9 counts them.
#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use vigia::{App, Body, Glyphs, Theme, View, body_layout, regions, render};
use vigia_core::{
    ChangeKind, FrameStats, HISTORY_PATHS, HighlightStats, Highlighter, History, HistoryStats,
    WARM_FILES, WatchOptions, WatchStats, Worktree,
};

/// Worktree to watch. Defaults to the checkout the test runs in.
const TREE: &str = "VIGIA_OBSERVE_TREE";
/// How long to watch for, in seconds.
const SECS: &str = "VIGIA_OBSERVE_SECS";
/// A path whose existence ends the run early, **with its report**.
///
/// A session cannot know its own length in advance, and the alternative is
/// killing the process, which loses everything it measured. Checked once per
/// sample rather than once per frame: a `stat` per frame would put a syscall on
/// the loop being measured, which is the shape §7 spends four bullets on.
const STOP: &str = "VIGIA_OBSERVE_STOP";

/// Window used when nothing asks for another, short enough that a mistaken
/// invocation is not a lost afternoon.
const DEFAULT_SECS: u64 = 120;

/// Samples across the window, whatever its length.
///
/// The soak's two numbers, deliberately: holding the *count* rather than the
/// interval is what lets a two-minute window and a four-hour one produce
/// comparable series, and this report exists to be read beside that one.
const MAX_SAMPLES: usize = 288;
const MIN_SAMPLES: usize = 12;

/// Upper bounds of the frame-time histogram, in microseconds.
///
/// **16000 is an exact edge, and that is the whole reason this is a histogram
/// with hand-written edges rather than a log scale.** #72 item 2 asks whether a
/// frame is *"ever observed above 16ms"*, which is I9's budget, so the answer
/// has to be a **count** and never an interpolation between two buckets that
/// straddle it. [`Histogram::at_or_over`] refuses any threshold that is not one
/// of these, so that property cannot quietly stop holding.
///
/// The rest sit where this repository's own measurements already are: about 3ms
/// for a settled reuse, 7 to 11ms for a real frame under continuous edits, 25 to
/// 61ms for a first entry into a large hunk. A boundary between two of those is
/// what makes the printed shape readable rather than merely present.
const EDGES: [u64; 17] = [
    250, 500, 750, 1_000, 1_500, 2_000, 3_000, 4_000, 6_000, 8_000, 12_000, 16_000, 24_000, 32_000,
    48_000, 64_000, 100_000,
];

/// I9's budget, and one of [`EDGES`].
const BUDGET_US: u64 = 16_000;

/// Worst frames kept, with the context that says what they were.
///
/// Bounded, and the bound is the point rather than an economy. A `Vec` of every
/// frame would be harness memory growing with the run **inside the process whose
/// memory is one of the measurements**, so the instrument would report its own
/// growth as the product's. The soak states the same rule for its path set.
const WORST: usize = 8;

/// Pane the session is measured at.
///
/// One geometry rather than the soak's rotation: a resize is something a reader
/// does, and this file measures a pane that is left alone. 80x24 is the size
/// `SPEC.md` §11.1 sizes the pinned file list against.
const AREA: (u16, u16) = (80, 24);

fn env_var<T: std::str::FromStr>(name: &str, fallback: T) -> T {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(fallback)
}

/// How many samples a window of this length gets. The soak's rule.
fn samples_for(window: Duration) -> usize {
    (window.as_secs() as usize / 2).clamp(MIN_SAMPLES, MAX_SAMPLES)
}

/// The tree to watch.
fn tree() -> PathBuf {
    PathBuf::from(std::env::var(TREE).unwrap_or_else(|_| ".".to_owned()))
}

/// What the header would draw on the left, which is the worktree's own name.
///
/// **`vigia::run`'s `short_name` in the same order, and the order is the whole
/// of it.** That function is private to the `vigia` crate and not re-exported,
/// so this cannot call it and has to agree with it by construction instead: the
/// last component first, the canonicalised last component second, and the path
/// itself rather than a placeholder when a worktree sits at a filesystem root
/// and has no last component by either route. An earlier version canonicalised
/// first and fell back to the literal `"worktree"`, which would have made the
/// report name a tree the header would have named something else, on exactly
/// the paths where the two routes disagree.
fn tree_name(tree: &Path) -> String {
    if let Some(name) = tree.file_name() {
        return name.to_string_lossy().into_owned();
    }
    if let Ok(resolved) = tree.canonicalize()
        && let Some(name) = resolved.file_name()
    {
        return name.to_string_lossy().into_owned();
    }
    tree.display().to_string()
}

// ---------------------------------------------------------------------------
// The frame-time histogram
// ---------------------------------------------------------------------------

/// Frame costs, bucketed rather than retained.
///
/// Bucket `i` holds `EDGES[i - 1] <= v < EDGES[i]`, bucket `0` holds
/// `v < EDGES[0]`, and the last holds everything at or above the final edge.
#[derive(Debug, Default, Clone)]
struct Histogram {
    counts: [u64; EDGES.len() + 1],
    total: u64,
    max: u64,
    /// Summed microseconds, which is what makes a mean available without
    /// keeping a sample.
    sum: u64,
}

/// Where a percentile fell, as the bucket that contains it.
///
/// An interval rather than a value, because that is what a histogram actually
/// knows. A single number here would be an interpolation wearing a
/// measurement's clothes, and this file exists to stop exactly that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Interval {
    low: u64,
    /// `None` for the open-topped last bucket, where the report prints
    /// [`Histogram::max`] instead.
    high: Option<u64>,
}

impl Histogram {
    fn record(&mut self, cost: Duration) {
        let us = cost.as_micros().min(u128::from(u64::MAX)) as u64;
        let bucket = EDGES
            .iter()
            .position(|&edge| us < edge)
            .unwrap_or(EDGES.len());
        self.counts[bucket] += 1;
        self.total += 1;
        self.max = self.max.max(us);
        self.sum = self.sum.saturating_add(us);
    }

    /// Frames at or over `edge`, which **must** be one of [`EDGES`].
    ///
    /// `None` for anything else, deliberately and loudly. A count over a
    /// threshold that is not a boundary is an estimate, and the one question
    /// this histogram exists to answer exactly is how many frames reached I9's
    /// budget.
    fn at_or_over(&self, edge: u64) -> Option<u64> {
        let first = EDGES.iter().position(|&e| e == edge)?;
        Some(self.counts[first + 1..].iter().sum())
    }

    /// The bucket holding the nearest-rank percentile.
    ///
    /// Nearest rank, matching `vigia_core::Samples` in the shell and the soak's
    /// own statistic: rank `ceil(fraction * n)`, one-based, no interpolation.
    fn rank(&self, fraction: f64) -> Option<Interval> {
        if self.total == 0 {
            return None;
        }
        let target = ((fraction * self.total as f64).ceil() as u64).max(1);
        let mut seen = 0;
        for (bucket, &count) in self.counts.iter().enumerate() {
            seen += count;
            if seen >= target {
                return Some(Interval {
                    low: if bucket == 0 { 0 } else { EDGES[bucket - 1] },
                    high: EDGES.get(bucket).copied(),
                });
            }
        }
        None
    }

    /// Whether a percentile at this count is just the maximum wearing a
    /// percentile's name.
    ///
    /// **Found by running the instrument rather than by writing it**, which is
    /// the reason this is a derived check and not a constant. The first real
    /// window drew 55 frames, cleared a 40-frame floor that looked generous, and
    /// printed a p99 of `>=100ms` that was the single 591.74ms frame in the run
    /// and nothing else. §7 already names the shape — *"at 30 samples a
    /// nearest-rank p99 is just the maximum"* — and it names it as one of the
    /// two flaws that were invisible to reading.
    ///
    /// The arithmetic decides it rather than taste: a nearest-rank p99 is
    /// `ceil(0.99n)`, so it stops being the top sample at **n = 100**, where it
    /// is rank 99 and one outlier is excluded. That is the same reasoning §5.1
    /// gives for the shell's own 128-frame ring. Below it the report prints the
    /// budget count and the maximum, which are exact at any count, and says
    /// there is no percentile rather than quoting one.
    fn rank_is_max(&self, fraction: f64) -> bool {
        self.total > 0 && ((fraction * self.total as f64).ceil() as u64).max(1) >= self.total
    }

    fn mean(&self) -> Option<u64> {
        (self.total > 0).then(|| self.sum / self.total)
    }

    /// The shape, as one field per non-empty bucket.
    fn curve(&self) -> String {
        self.counts
            .iter()
            .enumerate()
            .filter(|&(_, &count)| count > 0)
            .map(|(bucket, count)| match EDGES.get(bucket) {
                Some(edge) => format!("<{edge}us={count}"),
                None => format!(">={}us={count}", EDGES[EDGES.len() - 1]),
            })
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// One frame worth keeping, and what it was.
///
/// #72 item 2 asks not only whether a frame went over budget but *"during
/// what"*, and none of that is recoverable afterwards: the changed set moves,
/// the screen moves, and the tick that caused it is gone.
#[derive(Debug, Clone, Copy)]
struct Worst {
    cost: Duration,
    at: Duration,
    /// Changed files in the worktree when it was drawn.
    files: usize,
    /// Rows the diff region actually produced.
    rows: usize,
    /// Paths named by the tick that woke this frame.
    tick_paths: usize,
    /// Frames completed before it, so a cold first frame is legible as one.
    after: u64,
}

/// Keeps the worst [`WORST`] of an unbounded stream, biggest first.
fn offer(worst: &mut Vec<Worst>, candidate: Worst) {
    let at = worst
        .iter()
        .position(|held| candidate.cost > held.cost)
        .unwrap_or(worst.len());
    if at >= WORST {
        return;
    }
    worst.insert(at, candidate);
    worst.truncate(WORST);
}

// ---------------------------------------------------------------------------
// The RSS series
// ---------------------------------------------------------------------------

/// The median of a slice, by the soak's rule: the lower of the two middles on an
/// even count, so the answer is always a sample that was actually taken.
fn median(values: &[u64]) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    Some(sorted[(sorted.len() - 1) / 2])
}

/// Quarter medians over a series, in order.
///
/// **Deliberately not the soak's `quarter_medians`, and deliberately not shared
/// with it.** That one is half of a gate: its slicing, its warmup prefix and its
/// refusal floor are all part of what §7 and
/// [#126](https://github.com/breferrari/vigia/issues/126) ruled, so a second
/// copy reachable from another file would be a second definition of a gated
/// statistic. This one gates nothing and the line it prints says so. What it is
/// for is the **level**, which is what #72 item 3 asks for: where a real
/// session's RSS sits against the soak's plateau on a synthetic one. Drift stays
/// the soak's question, over the soak's window.
fn quarters(values: &[u64]) -> Option<[u64; 4]> {
    if values.len() < 4 {
        return None;
    }
    let step = values.len() / 4;
    let mut out = [0u64; 4];
    for (q, slot) in out.iter_mut().enumerate() {
        let start = q * step;
        let end = if q == 3 { values.len() } else { start + step };
        *slot = median(&values[start..end])?;
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// The oracle
// ---------------------------------------------------------------------------

/// One changed path, as either side counts it.
///
/// `added`/`removed` are `None` where the side declines to count, which is a
/// third state and not a zero: that is the distinction a `-` row in
/// `--numstat` exists to draw, and the one an eager parse loses. A binary file
/// counted as `+0 -0` would agree with a diff nobody took.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Row {
    path: String,
    added: Option<u32>,
    removed: Option<u32>,
}

/// `git diff --numstat -z`, parsed.
///
/// The `-z` form is three tab-separated fields per NUL-terminated record, except
/// for a rename or a copy, where the path field is **empty** and the preimage
/// and postimage follow as their own NUL-terminated records. Parsing the
/// non-`-z` form instead would need a rule for a path containing a tab, a quote
/// or a newline, and §7's fixture-axis rule is exactly about not taking a
/// position on something like that by accident.
fn parse_numstat(raw: &str) -> Vec<Row> {
    let mut fields = raw.split('\0');
    let mut rows = Vec::new();
    while let Some(record) = fields.next() {
        if record.is_empty() {
            continue;
        }
        let mut parts = record.splitn(3, '\t');
        let (Some(added), Some(removed), Some(path)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        // A rename or a copy. The **postimage** is the path both sides name,
        // which is what `ChangeKind::Renamed` carries as its own `path` with the
        // preimage in `from`.
        let path = if path.is_empty() {
            let _preimage = fields.next();
            match fields.next() {
                Some(postimage) => postimage.to_owned(),
                None => continue,
            }
        } else {
            path.to_owned()
        };
        rows.push(Row {
            path,
            added: added.parse().ok(),
            removed: removed.parse().ok(),
        });
    }
    rows.sort_by(|a, b| a.path.cmp(&b.path));
    rows
}

/// `git status --porcelain -z`, parsed to `(status, path)`.
///
/// The two status letters are kept rather than discarded, because the untracked
/// marker is the only trustworthy way to know that a path git's `diff` will
/// never mention is *expected* to be missing from it rather than a disagreement.
/// A rename record carries its preimage as an extra NUL-terminated field, and
/// that field is dropped: this is the set of paths a monitor draws a row for,
/// and a rename draws one row.
fn parse_porcelain(raw: &str) -> Vec<(String, String)> {
    let mut fields = raw.split('\0');
    let mut entries = Vec::new();
    while let Some(entry) = fields.next() {
        if entry.len() < 4 {
            continue;
        }
        let (status, path) = entry.split_at(3);
        let status = status[..2].to_owned();
        if status.contains('R') || status.contains('C') {
            let _preimage = fields.next();
        }
        entries.push((status, path.to_owned()));
    }
    entries.sort_by(|a, b| a.1.cmp(&b.1));
    entries
}

/// Where two descriptions of the same worktree disagree.
///
/// **Both directions, always.** A check between two collections has the
/// direction of whichever one it iterates, and a one-directional sweep reads
/// exactly like a two-directional one when it comes back clean. Ours missing a
/// path git has and ours holding a path git does not are different defects, and
/// the second is the one a monitor gets punished for.
fn disagreements(ours: &[Row], theirs: &[Row]) -> Vec<String> {
    let mine: BTreeMap<&str, &Row> = ours.iter().map(|row| (row.path.as_str(), row)).collect();
    let yours: BTreeMap<&str, &Row> = theirs.iter().map(|row| (row.path.as_str(), row)).collect();
    let mut out = Vec::new();
    for (path, row) in &mine {
        match yours.get(path) {
            None => out.push(format!("{path}: vigia has it, git does not")),
            Some(other) if other.added != row.added || other.removed != row.removed => {
                out.push(format!(
                    "{path}: vigia +{} -{}, git +{} -{}",
                    count(row.added),
                    count(row.removed),
                    count(other.added),
                    count(other.removed)
                ))
            }
            Some(_) => {}
        }
    }
    for path in yours.keys() {
        if !mine.contains_key(path) {
            out.push(format!("{path}: git has it, vigia does not"));
        }
    }
    out
}

/// Where two path sets disagree, in both directions and for the same reason.
fn missing(ours: &BTreeSet<String>, theirs: &BTreeSet<String>, what: &str) -> Vec<String> {
    let mut out: Vec<String> = ours
        .difference(theirs)
        .map(|path| format!("{path}: vigia lists it, {what} does not"))
        .collect();
    out.extend(
        theirs
            .difference(ours)
            .map(|path| format!("{path}: {what} lists it, vigia does not")),
    );
    out
}

/// Rows with the excluded paths dropped.
///
/// One function rather than the same predicate written twice, because it is
/// applied to **both** sides of the comparison and the two have to stay
/// identical: an exclusion that reached one side alone would turn a legitimate
/// asymmetry into a disagreement, or hide a real one.
fn excluding(rows: &[Row], excluded: &BTreeSet<String>) -> Vec<Row> {
    rows.iter()
        .filter(|row| !excluded.contains(&row.path))
        .cloned()
        .collect()
}

fn count(n: Option<u32>) -> String {
    match n {
        Some(n) => n.to_string(),
        None => "-".to_owned(),
    }
}

fn git(tree: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(tree)
        // The oracle is a reader too, and one that refreshed the index would be
        // changing the thing it is checking.
        .arg("--no-optional-locks")
        .args(args)
        .output()
        .expect("git is on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

// ---------------------------------------------------------------------------
// The measured loop
// ---------------------------------------------------------------------------

/// One reading, over the fields the soak samples, so the two reports compare.
struct Sample {
    at: Duration,
    rss: u64,
    /// What the **status bar** was showing at that moment, which is the number
    /// #72 item 2 names and the only one a reader glancing at the pane ever
    /// sees. A different statistic from the whole-session histogram and allowed
    /// to disagree with it: this is a p99 over the last 128 frames, so a breach
    /// rolls off it, which is the gap B7's decline predicted and left standing.
    readout: Option<Duration>,
    files: usize,
    tracked_diffs: usize,
    tracked_spans: usize,
    tracked_hunks: usize,
    tracked_history: usize,
    body: usize,
}

struct Report {
    tree: String,
    window: Duration,
    /// The interval the run *planned* to sample at.
    ///
    /// Carried rather than recomputed from the window and the samples taken.
    /// A run stopped early has fewer samples than it planned, so dividing the
    /// window by what arrived reported a 847-second nominal cadence beside a
    /// 50-second actual gap on the first real window, which reads as a stall and
    /// was the report dividing by the wrong denominator.
    step: Duration,
    elapsed: Duration,
    stopped_early: bool,
    samples: Vec<Sample>,
    frames: u64,
    ticks: u64,
    /// Sample intervals that passed with nothing written.
    ///
    /// On a fixture this would mean the writer stopped. Here it means the agent
    /// was thinking, which is I1 working rather than a failure, and how much of
    /// a session is idle is itself a number nobody has.
    idle_waits: u64,
    failed: u64,
    last_error: Option<String>,
    hist: Histogram,
    worst: Vec<Worst>,
    watch: WatchStats,
    frame: FrameStats,
    highlight: HighlightStats,
    history: HistoryStats,
}

const MIB: f64 = 1024.0 * 1024.0;

fn mib(bytes: u64) -> f64 {
    bytes as f64 / MIB
}

fn ms(us: u64) -> f64 {
    us as f64 / 1000.0
}

fn interval(interval: Option<Interval>) -> String {
    match interval {
        Some(Interval {
            low,
            high: Some(high),
        }) => format!("{:.2}-{:.2}ms", ms(low), ms(high)),
        Some(Interval { low, high: None }) => format!(">={:.2}ms", ms(low)),
        None => "none".to_owned(),
    }
}

impl Report {
    fn rss(&self) -> Vec<u64> {
        self.samples.iter().map(|s| s.rss).collect()
    }

    /// Every line of the report, built rather than printed.
    ///
    /// Built, because the report is the entire output of an instrument that
    /// asserts nothing else, which makes it the one thing about a run that *can*
    /// be gated. That is what the plan asked for and what the quiet-session gate
    /// below spends: a run that measured almost nothing has to reach a reader as
    /// "no percentile here" rather than as a percentile over six frames, and
    /// that is a property of the text rather than of the histogram.
    fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "observe: tree {}, window {:?}, ran {:?}{}, {} samples, {} frames, {} ticks, \
             {} idle waits",
            self.tree,
            self.window,
            self.elapsed,
            if self.stopped_early {
                " (stopped early)"
            } else {
                ""
            },
            self.samples.len(),
            self.frames,
            self.ticks,
            self.idle_waits,
        ));
        // Whether the series below is evenly spaced, which nothing else on this
        // page says. The soak learned the same thing one statistic over: a
        // sample index is only a clock while the loop keeps up, and a series
        // read as evenly spaced when it was not is wrong in whichever direction
        // the stall happened to fall.
        match self
            .samples
            .windows(2)
            .map(|pair| pair[1].at.saturating_sub(pair[0].at))
            .max()
        {
            Some(widest) => out.push(format!(
                "observe: sample cadence nominal {:?}, widest actual gap {:?}",
                self.step, widest
            )),
            None => out.push("observe: sample cadence has one sample or none".to_owned()),
        }

        // The budget count first, because it is the question, and it is exact at
        // any frame count: a count over a declared edge needs no percentile.
        out.push(format!(
            "observe: frames at or over I9's {:.0}ms budget: {} of {}, max {:.2}ms, \
             mean {:.2}ms",
            ms(BUDGET_US),
            self.hist
                .at_or_over(BUDGET_US)
                .map(|over| over.to_string())
                .unwrap_or_else(|| "?".to_owned()),
            self.hist.total,
            ms(self.hist.max),
            self.hist.mean().map(ms).unwrap_or(0.0),
        ));
        // The percentiles, only where they are percentiles. See `rank_is_max`.
        if self.hist.rank_is_max(0.99) {
            out.push(format!(
                "observe: {} frames is too few for a p99, which at this count is the \
                 maximum above; no percentile is quoted",
                self.hist.total,
            ));
        } else {
            out.push(format!(
                "observe: frame p50 {}, p99 {} (reported, not gated)",
                interval(self.hist.rank(0.50)),
                interval(self.hist.rank(0.99)),
            ));
        }
        out.push(format!("observe: frame curve {}", self.hist.curve()));

        // The readout a reader would actually have caught.
        let readouts: Vec<u64> = self
            .samples
            .iter()
            .filter_map(|s| s.readout)
            .map(|d| d.as_micros() as u64)
            .collect();
        match (readouts.iter().max(), median(&readouts)) {
            (Some(&worst), Some(mid)) => out.push(format!(
                "observe: status-bar p99 readout worst {:.2}ms, median {:.2}ms, over {} samples",
                ms(worst),
                ms(mid),
                readouts.len()
            )),
            _ => out.push("observe: status-bar p99 readout never populated".to_owned()),
        }

        for (rank, worst) in self.worst.iter().enumerate() {
            out.push(format!(
                "observe: worst frame {}: {:.2}ms at {:?}, {} changed files, {} rows, \
                 {} paths in the tick, after {} frames",
                rank + 1,
                worst.cost.as_secs_f64() * 1000.0,
                worst.at,
                worst.files,
                worst.rows,
                worst.tick_paths,
                worst.after,
            ));
        }

        // The level, never a drift verdict. See `quarters`.
        let rss = self.rss();
        match (rss.first(), rss.iter().max(), quarters(&rss)) {
            (Some(&first), Some(&peak), Some(q)) => out.push(format!(
                "observe: rss first {:.2} MiB, peak {:.2}, quarters {:.2}, {:.2}, {:.2}, \
                 {:.2} MiB (level only: drift is the soak's gate, over the soak's window)",
                mib(first),
                mib(peak),
                mib(q[0]),
                mib(q[1]),
                mib(q[2]),
                mib(q[3]),
            )),
            _ => out.push(format!(
                "observe: rss from {} samples is too few for a level",
                rss.len()
            )),
        }

        out.push(format!(
            "observe: changed files min {}, max {}; body rows max {}",
            self.samples.iter().map(|s| s.files).min().unwrap_or(0),
            self.samples.iter().map(|s| s.files).max().unwrap_or(0),
            self.samples.iter().map(|s| s.body).max().unwrap_or(0),
        ));
        out.push(format!(
            "observe: tracked diffs max {}, heights max {}, hunks max {}, history max {} of {}",
            self.samples
                .iter()
                .map(|s| s.tracked_diffs)
                .max()
                .unwrap_or(0),
            self.samples
                .iter()
                .map(|s| s.tracked_spans)
                .max()
                .unwrap_or(0),
            self.samples
                .iter()
                .map(|s| s.tracked_hunks)
                .max()
                .unwrap_or(0),
            self.samples
                .iter()
                .map(|s| s.tracked_history)
                .max()
                .unwrap_or(0),
            HISTORY_PATHS,
        ));
        // I1's own counters over a real tree, and `filtered` is the figure with
        // no fixture behind it anywhere: a synthetic worktree has no `target/`
        // in it, so nothing in this repository has ever measured what an ignored
        // subtree under a running build costs the watch.
        out.push(format!(
            "observe: watch wakeups {}, filtered {}, ticks {} ({:.1}% of wakeups filtered)",
            self.watch.wakeups,
            self.watch.filtered,
            self.watch.ticks,
            100.0 * self.watch.filtered as f64 / self.watch.wakeups.max(1) as f64,
        ));
        out.push(format!(
            "observe: frame computed {}, reused {}, measured {}, probes {}, {:.2} MiB compared",
            self.frame.computed,
            self.frame.reused,
            self.frame.measured,
            self.frame.probes,
            mib(self.frame.bytes),
        ));
        out.push(format!(
            "observe: highlight parsed {}, reused {}, evicted {}, {} lines",
            self.highlight.parsed,
            self.highlight.reused,
            self.highlight.evicted,
            self.highlight.lines,
        ));
        out.push(format!(
            "observe: history recorded {}, evicted {} by cap and {} by window",
            self.history.recorded, self.history.evicted_by_cap, self.history.evicted_by_window,
        ));
        out.push(format!(
            "observe: failed frames {} of {}{}",
            self.failed,
            self.frames,
            match &self.last_error {
                Some(e) => format!(", last: {e}"),
                None => String::new(),
            }
        ));
        // From the series already in hand rather than from a second walk of
        // `samples`: one traversal, one meaning.
        let curve: Vec<String> = rss.iter().map(|bytes| (bytes / 1024).to_string()).collect();
        out.push(format!("observe: rss KiB = {}", curve.join(",")));
        let files: Vec<String> = self.samples.iter().map(|s| s.files.to_string()).collect();
        out.push(format!("observe: changed files = {}", files.join(",")));
        out
    }

    fn print(&self) {
        for line in self.lines() {
            println!("{line}");
        }
    }
}

/// Everything the shell holds between frames, which `vigia::run` calls `Shell`.
///
/// A struct rather than a dozen locals threaded through a paint function, for
/// the reason the product has one: a paint reads and writes most of it, and the
/// alternative is a thirteen-parameter call that nothing can keep in agreement
/// with the loop around it.
struct Pane<'w> {
    app: App,
    frame: vigia_core::Frame<'w>,
    highlighter: Highlighter,
    history: History,
    theme: Theme,
    area: Rect,
    buffer: Buffer,
    view: View,
    body: Body,
    /// What the header draws on the left. See [`tree_name`].
    name: String,
    /// The branch, when the empty state is the thing being drawn.
    branch: Option<String>,
    failed: u64,
    last_error: Option<String>,
}

impl Pane<'_> {
    /// `Shell::draw`: one paint, and a second when the first left a debt.
    ///
    /// **The debt is I7's whole mechanism.** The first frame draws plain and
    /// owes its colour to the frame after it, because a grammar's patterns
    /// compile on first use at 74-362ms and that is not a cost to put on the
    /// frame a reader is waiting for (§6, §3's I7 row). A harness that painted
    /// once would time the plain half of a frame the reader sees painted twice,
    /// and would report the cheaper half as the frame.
    fn draw(&mut self, worktree: &Worktree) {
        self.paint(worktree);
        if self.app.owes_repaint() {
            self.paint(worktree);
        }
    }

    /// One collect and one paint, in the order `Shell::paint` performs them.
    ///
    /// Three stages here are ones an earlier version of this file left out, and
    /// each of them is a cost the report would otherwise have under-stated.
    /// §7's own rule is that a gate is written against the caller's **whole**
    /// frame and any stage left outside it is a stage nothing can regress you
    /// on; the same arithmetic applies to an instrument, one step further along,
    /// because a number is only comparable to the product's if it was taken over
    /// the product's frame.
    ///
    /// - The **branch read**, one `.git/HEAD` at 56 to 69us. It used to happen
    ///   on exactly the frames that drew the empty state, which over the
    ///   recorded sessions was about half of them; since
    ///   [#158](https://github.com/breferrari/vigia/issues/158) the header draws
    ///   the branch always, so it happens on all of them and this instrument
    ///   would under-state the frame by that much if it were left out.
    /// - The **second** `chrome`, rebuilt after the collect so a notice raised
    ///   during it reaches this frame rather than the next one.
    /// - [`regions`], which resolves the clickable areas from the drawn frame.
    ///   It is inside the product's own draw closure and so inside its timed
    ///   region.
    ///
    /// What stays out, named rather than discovered later: the terminal
    /// takeover and the real `ratatui` terminal, which need a tty and are I8's;
    /// the input thread, because there is no reader; and the signal handler,
    /// which is process-global and would arm this harness rather than a shell.
    /// The buffer is a real one and `render` writes into it.
    fn paint(&mut self, worktree: &Worktree) {
        // Every frame, matching `Shell::paint` since #158. This went through a
        // `branch_for` seam whose guard was *only the empty state names a
        // branch*; the header names it always, so the guard's premise went and
        // the wrapper with it.
        self.branch = worktree.branch();
        let chrome = self
            .app
            .chrome(&self.name, self.branch.as_deref(), None, None, None, None);
        self.body = body_layout(self.area, &chrome, self.frame.files().len());
        match self.app.view(
            &mut self.frame,
            &mut self.highlighter,
            &self.history,
            self.body,
        ) {
            Ok(fresh) => self.view = fresh,
            Err(e) => {
                self.failed += 1;
                self.last_error = Some(e.to_string());
                // The product warns rather than dies, and the notice it raises
                // is what the rebuilt chrome below exists to carry.
                self.app.warn(e.to_string());
            }
        }
        let chrome = self
            .app
            .chrome(&self.name, self.branch.as_deref(), None, None, None, None);
        let _regions = regions(self.area, &chrome, &self.view);
        render(
            &mut self.buffer,
            self.area,
            &self.view,
            &self.theme,
            Glyphs::default(),
            &chrome,
        );
    }

    /// The status bar's own p99 readout, as a reader glancing at the pane would
    /// see it.
    fn readout(&self) -> Option<Duration> {
        self.app
            .chrome(&self.name, self.branch.as_deref(), None, None, None, None)
            .frame
    }
}

impl<'w> Pane<'w> {
    /// Everything `vigia::run` builds before its first wake.
    ///
    /// An associated function rather than the body of [`drive`], so that
    /// [`wiring`] can construct exactly what a session runs and drive one frame
    /// through it without a watch, a window or a writer.
    fn open(worktree: &'w Worktree, name: String) -> Self {
        let mut frame = worktree.frame();
        frame.advance().expect("the first walk");

        let highlighter = Highlighter::new();
        // The warmer, because `run` spawns one and this claims to be `run` with
        // the terminal taken out. Joined rather than detached, so a one-off
        // startup cost cannot land inside the first sample. The soak gives the
        // same reason.
        highlighter
            .warm_ahead(
                worktree.workdir().to_path_buf(),
                frame
                    .files()
                    .iter()
                    .take(WARM_FILES)
                    .map(|change| change.path.clone())
                    .collect(),
            )
            .join()
            .expect("the warmer thread");

        let area = Rect::new(0, 0, AREA.0, AREA.1);
        Self {
            app: App::new(),
            frame,
            highlighter,
            history: History::new(),
            theme: Theme::default(),
            area,
            buffer: Buffer::empty(area),
            view: View::default(),
            body: Body::default(),
            name,
            branch: None,
            failed: 0,
            last_error: None,
        }
    }
}

/// The loop, which is `vigia::run` with the terminal and the reader taken out.
///
/// Everything the shell does between waking and drawing, in the order it does
/// it, and nothing else. There is no writer, no scripted input and no resize:
/// each of those is something an agent or a reader does, and this measures a
/// pane sitting beside one.
fn drive(tree: &Path, window: Duration, rx: &mpsc::Receiver<Vec<String>>) -> Report {
    let name = tree_name(tree);
    let worktree = Worktree::discover(tree).expect("discover the worktree under observation");
    let mut pane = Pane::open(&worktree, name.clone());

    let stop = std::env::var(STOP).ok().map(PathBuf::from);
    let count = samples_for(window);
    let step = window / count as u32;
    let started = Instant::now();

    let mut samples = Vec::with_capacity(count);
    let (mut frames, mut ticks, mut idle_waits) = (0u64, 0u64, 0u64);
    let mut hist = Histogram::default();
    let mut worst: Vec<Worst> = Vec::with_capacity(WORST + 1);
    let mut stopped_early = false;

    while samples.len() < count {
        let deadline = started + step * (samples.len() as u32 + 1);
        let now = Instant::now();
        if now >= deadline {
            samples.push(Sample {
                at: started.elapsed(),
                rss: vigia::memory::resident().unwrap_or(0),
                readout: pane.readout(),
                files: pane.frame.files().len(),
                tracked_diffs: pane.frame.tracked(),
                tracked_spans: pane.frame.tracked_spans(),
                tracked_hunks: pane.highlighter.tracked(),
                tracked_history: pane.history.tracked(),
                body: pane.body.diff,
            });
            // Once per sample, never per frame: see `STOP`.
            if stop.as_deref().is_some_and(Path::exists) {
                stopped_early = true;
                break;
            }
            continue;
        }

        let tick_paths = match rx.recv_timeout(deadline - now) {
            Err(RecvTimeoutError::Timeout) => {
                idle_waits += 1;
                continue;
            }
            // The watcher gave up. Unlike the soak this is not a panic, because
            // the run has real numbers behind it by then and losing them to
            // prove a point about the watch would be the wrong trade. The report
            // carries it as the last error and `ticks` says how far it got.
            Err(RecvTimeoutError::Disconnected) => {
                pane.last_error = Some("the watch thread ended before the window did".to_owned());
                break;
            }
            Ok(paths) => {
                ticks += 1;
                // Sampled on the wake, before the walk, exactly where
                // `vigia::run` samples it.
                pane.history
                    .record(paths.iter().map(String::as_str), Instant::now());
                // Advance first, follow second: the path is looked up in the
                // file list, and before the walk that list is the previous
                // frame's.
                match pane.frame.advance() {
                    Ok(()) => {
                        if let Some(path) = paths.last() {
                            let followed = path.clone();
                            pane.app.follow(&followed, &pane.frame);
                        }
                    }
                    Err(e) => {
                        pane.failed += 1;
                        pane.last_error = Some(e.to_string());
                    }
                }
                paths.len()
            }
        };

        let began = Instant::now();
        // Before the paint and inside the timed region, exactly where
        // `vigia::run` puts it: a readout measured outside the thing it reports
        // is the omission §7 names.
        pane.app.sample_memory();
        pane.draw(&worktree);
        let cost = began.elapsed();
        pane.app.record_frame(cost);
        hist.record(cost);
        offer(
            &mut worst,
            Worst {
                cost,
                at: started.elapsed(),
                files: pane.frame.files().len(),
                rows: pane.view.rows.len(),
                tick_paths,
                after: frames,
            },
        );
        frames += 1;
    }

    Report {
        tree: name,
        window,
        step,
        elapsed: started.elapsed(),
        stopped_early,
        samples,
        frames,
        ticks,
        idle_waits,
        failed: pane.failed,
        last_error: pane.last_error,
        hist,
        worst,
        // Filled by the caller, which is the only place the watcher lives.
        watch: WatchStats::default(),
        frame: pane.frame.stats(),
        highlight: pane.highlighter.stats(),
        history: pane.history.stats(),
    }
}

/// #72 items 1 to 3: what a real session costs the process a reader leaves open.
///
/// `#[ignore]` because it watches for minutes or hours against a tree the
/// environment names, and because it asserts nothing.
///
/// **Run it as the built test binary, copied out of `target/` first.** Two
/// separate frictions, and the second cost a window before it was understood.
/// Through `cargo test` a cargo process holds the `target/` lock for its whole
/// life, so the session being measured cannot build. And a running executable
/// is open, so leaving it *in* `target/` makes the next `cargo test --release`
/// fail to link over it (`LNK1104` on Windows, `ETXTBSY` on Linux) with an error
/// that names the linker rather than the run. Copy it somewhere else and point
/// the environment at the tree:
///
/// ```sh
/// cargo test --release --test observe --no-run
/// cp target/release/deps/observe-*.exe /somewhere/else/
/// VIGIA_OBSERVE_TREE=/path/to/worktree VIGIA_OBSERVE_SECS=7200 \
///   VIGIA_OBSERVE_STOP=/somewhere/else/stop /somewhere/else/observe.exe \
///   --ignored --nocapture observe_a_working_session
/// ```
#[test]
#[ignore = "an instrument: it watches a real worktree for as long as it is asked to"]
fn observe_a_working_session() {
    observe(&tree(), Duration::from_secs(env_var(SECS, DEFAULT_SECS))).print();
}

/// The watch, the loop, and the counters that only the watcher has.
///
/// Separate from the test above so that what a run *is* has one name, and
/// separate from [`drive`] because the watcher is the one part that has to live
/// on its own thread: `gix::Repository` is `Send` and not `Sync`, so a borrow of
/// one cannot cross a thread boundary. `vigia::run` and the soak both take the
/// same shape for the same reason.
fn observe(tree: &Path, window: Duration) -> Report {
    let (tx, rx) = mpsc::channel::<Vec<String>>();
    let (armed, arming) = mpsc::channel();
    let (counted, counting) = mpsc::channel();
    let root = tree.to_path_buf();

    let watch = std::thread::spawn(move || {
        let worktree = Worktree::discover(&root).expect("discover for the watch thread");
        let mut watcher = worktree
            .watch(WatchOptions::default())
            .expect("arm the watch");
        // The stopper leaves before the first tick can. A blocked `next_tick`
        // has nothing to interrupt it, so without this the join below waits for
        // an event that a finished session will never produce.
        let _ = armed.send(watcher.stopper());
        while let Some(tick) = watcher.next_tick() {
            if tx.send(tick.paths).is_err() {
                break;
            }
        }
        let _ = counted.send(watcher.stats());
    });

    let stopper = arming.recv().expect("the watch arms before it ticks");
    let mut report = drive(tree, window, &rx);
    stopper.stop();
    watch.join().expect("the watch thread");
    report.watch = counting.recv().unwrap_or_default();
    report
}

/// #72 item 4: any disagreement with `git diff`.
///
/// One sweep, in its own process, against its own [`Worktree`], for the reason
/// the module header gives. Run it on a timer from the shell rather than from
/// inside the loop above: a timer is the shell's to own here exactly as it is
/// B7 point 2's, and putting one inside the measured process would be inventing
/// the wake I1 forbids in the one place nobody would look for it.
#[test]
#[ignore = "an instrument: it sweeps a real worktree against git"]
fn observe_the_diff_against_git() {
    let tree = tree();
    let worktree = Worktree::discover(&tree).expect("discover the worktree under observation");
    let mut frame = worktree.frame();
    frame.advance().expect("the walk");

    let listed: Vec<(String, ChangeKind)> = frame
        .files()
        .iter()
        .map(|change| (change.path.clone(), change.kind.clone()))
        .collect();

    let mut ours = Vec::new();
    // A conflict or a type change is a real change with no line diff, and git
    // counts something for one. Named rather than compared: reporting it as a
    // disagreement every sweep would train a reader to skip the list, which is
    // how a real finding gets lost.
    let mut undiffable = Vec::new();
    for (index, (path, kind)) in listed.iter().enumerate() {
        if matches!(kind, ChangeKind::Conflict | ChangeKind::TypeChange) {
            undiffable.push(path.clone());
            continue;
        }
        let (_, diff) = frame.diff(index).expect("diff a changed file");
        ours.push(Row {
            path: path.clone(),
            added: (!diff.binary).then_some(diff.added),
            removed: (!diff.binary).then_some(diff.removed),
        });
    }

    let numstat = parse_numstat(&git(&tree, &["diff", "--numstat", "-z"]));
    let porcelain = parse_porcelain(&git(&tree, &["status", "--porcelain", "-z"]));

    // Untracked is read off git's own marker rather than inferred from absence
    // in `numstat`. Inferring it would classify "vigia lists a path git has
    // never heard of" as expected, which is the defect this sweep exists to
    // find, laundered by the code looking for it.
    let untracked: BTreeSet<String> = porcelain
        .iter()
        .filter(|(status, _)| status.starts_with('?'))
        .map(|(_, path)| path.clone())
        .collect();
    // Excluded from the **counts** comparison on both sides, so the exclusion
    // cannot hide a one-sided disagreement. `git diff` compares the index
    // against the worktree and an untracked file is in neither side of it,
    // while `SPEC.md` §11.1 makes including them load-bearing here: that
    // asymmetry is correct rather than a finding. The path set below still
    // covers every one of them.
    let excluded: BTreeSet<String> = untracked
        .iter()
        .cloned()
        .chain(undiffable.iter().cloned())
        .collect();
    let mine = excluding(&ours, &excluded);
    let theirs = excluding(&numstat, &excluded);

    let counts = disagreements(&mine, &theirs);
    let listed_paths: BTreeSet<String> = listed.iter().map(|(path, _)| path.clone()).collect();
    let status_paths: BTreeSet<String> = porcelain.iter().map(|(_, path)| path.clone()).collect();
    let set = missing(&listed_paths, &status_paths, "git status");

    println!(
        "oracle: {} changed paths, {} compared against numstat, {} untracked, \
         {} not line-diffable",
        listed_paths.len(),
        mine.len(),
        untracked.len(),
        undiffable.len(),
    );
    if !undiffable.is_empty() {
        println!("oracle: not line-diffable: {}", undiffable.join(", "));
    }
    for line in counts.iter().chain(set.iter()) {
        println!("oracle: DISAGREEMENT {line}");
    }
    if counts.is_empty() && set.is_empty() {
        println!("oracle: agrees with git on every changed path");
    }
}

// ---------------------------------------------------------------------------
// What `cargo test` carries
// ---------------------------------------------------------------------------

/// The one gate that is not over a pure function, because the one mechanism in
/// this file that is not a pure function had nothing holding it.
///
/// [`Pane::draw`] settles the repaint debt I7 leaves after a plain first frame,
/// and `SPEC.md` §7 records what that mechanism is worth: in the product,
/// *"deleting the second statement left the whole suite green"*. This file
/// copied the sequence a second time and got it **wrong** on the first attempt,
/// which is the strongest possible argument for the gate: without the repaint,
/// the harness timed the plain half of a frame the reader sees painted twice,
/// and displaced its colour onto the next tick. Measured on the probe fixture
/// before and after: a first frame of 5.52ms became **558.83ms**, which is the
/// grammar compile arriving where the reader actually pays it.
///
/// Deliberately not over the watch, the window or a writer. A gate that waited
/// for a filesystem event inside a one-second window would be timing-dependent
/// on a loaded machine, and [#36](https://github.com/breferrari/vigia/issues/36)
/// is this repository's open record of what that costs. One `draw` over a real
/// worktree needs none of it.
mod wiring {
    use super::*;
    use support::Scratch;

    #[test]
    fn one_draw_settles_the_repaint_debt_and_colours_the_frame() {
        let scratch = Scratch::new("observe-wiring");
        scratch.write("src/main.rs", "fn main() {\n    let x = 1;\n}\n");
        scratch.commit_all("base");
        scratch.write(
            "src/main.rs",
            "fn main() {\n    let x = 2;\n    let y = 3;\n}\n",
        );

        let worktree = scratch.worktree();
        let mut pane = Pane::open(&worktree, "observe-wiring".to_owned());
        assert!(
            !pane.app.owes_repaint(),
            "nothing is owed before the first collect"
        );

        pane.draw(&worktree);

        assert_eq!(pane.failed, 0, "the draw failed: {:?}", pane.last_error);
        assert!(
            !pane.app.owes_repaint(),
            "one draw must settle the debt it raises, or the harness times the \
             plain half of a frame the reader sees painted twice"
        );
        assert!(
            pane.highlighter.stats().lines > 0,
            "the coloured pass never ran, so this frame is the plain one and the \
             grammar compile landed on some later frame instead"
        );
        assert!(
            !pane.view.rows.is_empty(),
            "a frame over a changed file drew nothing, so what the two \
             assertions above measured was an empty screen"
        );
    }

    /// The other half, and it exists because a mutation survived the one above.
    ///
    /// **The reason changed with [#158](https://github.com/breferrari/vigia/issues/158)
    /// and the gate is worth more, not less.** The read used to happen on exactly
    /// the frames that drew the empty state, so a dirty fixture could not reach
    /// it and deleting the read outright left the gate above green: it took a
    /// clean fixture to hold a cost the harness would otherwise under-measure on
    /// about half the recorded sessions.
    ///
    /// The header draws the branch on every frame now, so **both** fixtures pay
    /// it, and this asserts the pair rather than the clean case alone. That is
    /// the same "span the axis rather than move along it" answer §7 reaches
    /// twice, and spanning it is what would catch a future guard being
    /// reintroduced on one side.
    #[test]
    fn every_tree_draws_its_branch_because_the_header_names_it() {
        for (name, dirty) in [
            ("observe-branch-clean", false),
            ("observe-branch-dirty", true),
        ] {
            let scratch = Scratch::new(name);
            scratch.write("src/main.rs", "fn main() {}\n");
            scratch.commit_all("base");
            if dirty {
                scratch.write("src/main.rs", "fn main() { let x = 1; }\n");
            }

            let worktree = scratch.worktree();
            let mut pane = Pane::open(&worktree, name.to_owned());
            assert_eq!(
                pane.frame.files().is_empty(),
                !dirty,
                "{name} is not in the state this case is about"
            );

            pane.draw(&worktree);

            assert_eq!(
                pane.failed, 0,
                "{name}: the draw failed: {:?}",
                pane.last_error
            );
            assert_eq!(
                pane.branch,
                worktree.branch(),
                "{name}: the header names the branch on every frame, so every \
                 frame pays the read, and a harness that skipped it would \
                 under-measure them"
            );
            assert!(
                pane.branch.is_some(),
                "{name}: the fixture has a commit on a named branch, so a `None` \
                 here means the assertion above compared two nothings"
            );
        }
    }
}

/// Gates over the pure functions the two instruments draw their conclusions
/// from.
///
/// `SPEC.md` §7: *"Gates are mutation tested before they are trusted."* Every
/// one of these was broken deliberately and confirmed red before it was kept,
/// and the last pair is why: a check that can only ever report a disagreement is
/// as useless as one that can never report one, and only the second failure mode
/// is loud.
mod statistic {
    use super::*;

    fn hist_of(us: &[u64]) -> Histogram {
        let mut hist = Histogram::default();
        for &v in us {
            hist.record(Duration::from_micros(v));
        }
        hist
    }

    fn frame(us: u64) -> Worst {
        Worst {
            cost: Duration::from_micros(us),
            at: Duration::ZERO,
            files: 0,
            rows: 0,
            tick_paths: 0,
            after: 0,
        }
    }

    #[test]
    fn a_sample_lands_in_the_bucket_whose_upper_bound_it_is_under() {
        let hist = hist_of(&[0, 249, 250, 999, 1_000, 15_999, 16_000, 1_000_000]);
        assert_eq!(hist.counts[0], 2, "0 and 249 are under the first edge");
        assert_eq!(hist.total, 8);
        assert_eq!(hist.max, 1_000_000);
        assert_eq!(
            hist.counts[EDGES.len()],
            1,
            "a second-scale outlier lands in the open-topped bucket rather than \
             being clamped into the one below it"
        );
        assert_eq!(hist.counts.iter().sum::<u64>(), hist.total);
    }

    #[test]
    fn the_budget_edge_is_a_boundary_so_over_budget_is_a_count_rather_than_an_estimate() {
        let hist = hist_of(&[15_999, 16_000, 16_001, 500_000]);
        assert_eq!(
            hist.at_or_over(BUDGET_US),
            Some(3),
            "16000us itself is at budget and counts; 15999 does not"
        );
        assert_eq!(
            hist.at_or_over(BUDGET_US + 1),
            None,
            "a threshold that is not a bucket edge has no exact answer here, and \
             refusing is the only honest one"
        );
    }

    #[test]
    fn a_percentile_reports_an_interval_and_the_interval_contains_the_true_value() {
        // Ninety-nine cheap frames and one expensive one, so a nearest-rank p99
        // is the ninety-ninth value and p100 is the outlier.
        let mut values: Vec<u64> = vec![800; 99];
        values.push(50_000);
        let hist = hist_of(&values);

        let mut sorted = values.clone();
        sorted.sort_unstable();
        let true_p99 = sorted[((0.99 * 100.0f64).ceil() as usize) - 1];

        let got = hist.rank(0.99).expect("a percentile over 100 samples");
        assert!(
            got.low <= true_p99 && got.high.is_some_and(|high| true_p99 < high),
            "the true p99 {true_p99} is not inside {got:?}"
        );
        assert_eq!(
            hist.rank(1.0),
            Some(Interval {
                low: 48_000,
                high: Some(64_000)
            }),
            "the top rank is the bucket the outlier fell in"
        );
        assert_eq!(hist_of(&[]).rank(0.99), None, "no frames, no percentile");
    }

    #[test]
    fn a_p99_is_the_maximum_below_a_hundred_frames_and_the_report_is_told_so() {
        // The boundary is arithmetic, not taste: `ceil(0.99n)` reaches `n` for
        // every count under 100 and is 99 at 100, where exactly one outlier is
        // excluded. This is the check the first real window needed and did not
        // have: 55 frames cleared a 40-frame floor and printed a p99 that was
        // the run's single 591.74ms frame.
        assert!(hist_of(&[800; 99]).rank_is_max(0.99), "99 frames");
        assert!(!hist_of(&[800; 100]).rank_is_max(0.99), "100 frames");
        assert!(
            hist_of(&[800; 55]).rank_is_max(0.99),
            "the window that found it"
        );
        assert!(
            !hist_of(&[]).rank_is_max(0.99),
            "no frames is not a maximum"
        );
        // A p50 is a percentile long before a p99 is, and conflating the two
        // would suppress the one figure a quiet session does have.
        assert!(!hist_of(&[800; 55]).rank_is_max(0.50));
    }

    #[test]
    fn the_retained_worst_frames_keep_the_worst_frame() {
        // The maximum arrives first, in the middle, and last. A bounded keeper
        // that only ever compares against its own tail loses it in one of the
        // three, and a stream that happens to arrive sorted hides that.
        for at in [0usize, 20, 39] {
            let mut costs: Vec<u64> = (1..=40).collect();
            costs[at] = 9_999;
            let mut worst = Vec::new();
            for cost in costs {
                offer(&mut worst, frame(cost));
            }
            assert_eq!(worst.len(), WORST, "the keeper is bounded");
            assert_eq!(
                worst[0].cost,
                Duration::from_micros(9_999),
                "the worst frame survived arriving at index {at}"
            );
            assert!(
                worst.windows(2).all(|pair| pair[0].cost >= pair[1].cost),
                "kept biggest first"
            );
        }
    }

    #[test]
    fn a_binary_numstat_row_reads_as_binary_rather_than_as_no_change() {
        assert_eq!(
            parse_numstat("-\t-\tassets/preview.png\u{0}"),
            vec![Row {
                path: "assets/preview.png".to_owned(),
                added: None,
                removed: None,
            }],
            "a `-` is a refusal to count and must not become a zero, or every \
             binary file agrees with a diff nobody took"
        );
    }

    #[test]
    fn a_renamed_numstat_row_names_the_new_path_and_the_record_after_it_survives() {
        assert_eq!(
            parse_numstat("3\t1\t\u{0}src/old.rs\u{0}src/new.rs\u{0}4\t0\tsrc/other.rs\u{0}"),
            vec![
                Row {
                    path: "src/new.rs".to_owned(),
                    added: Some(3),
                    removed: Some(1)
                },
                Row {
                    path: "src/other.rs".to_owned(),
                    added: Some(4),
                    removed: Some(0)
                },
            ],
            "the postimage is the path both sides name, and consuming the pair \
             must not eat the record behind it"
        );
    }

    #[test]
    fn porcelain_keeps_the_status_drops_a_renames_preimage_and_keeps_untracked() {
        assert_eq!(
            parse_porcelain("R  src/new.rs\u{0}src/old.rs\u{0}?? scratch.txt\u{0} M SPEC.md\u{0}"),
            vec![
                (" M".to_owned(), "SPEC.md".to_owned()),
                ("??".to_owned(), "scratch.txt".to_owned()),
                ("R ".to_owned(), "src/new.rs".to_owned()),
            ],
            "a rename is one row, an untracked file is one this monitor draws, \
             and the marker survives because the untracked set is read off it"
        );
    }

    #[test]
    fn a_disagreement_is_reported_in_both_directions() {
        let ours = vec![
            Row {
                path: "a".to_owned(),
                added: Some(1),
                removed: Some(0),
            },
            Row {
                path: "only-ours".to_owned(),
                added: Some(2),
                removed: Some(2),
            },
        ];
        let theirs = vec![
            Row {
                path: "a".to_owned(),
                added: Some(9),
                removed: Some(0),
            },
            Row {
                path: "only-theirs".to_owned(),
                added: Some(1),
                removed: Some(1),
            },
        ];
        let found = disagreements(&ours, &theirs);
        assert_eq!(found.len(), 3, "one count and one path each way: {found:?}");
        assert!(found.iter().any(|line| line == "a: vigia +1 -0, git +9 -0"));
        assert!(
            found
                .iter()
                .any(|line| line == "only-ours: vigia has it, git does not")
        );
        assert!(
            found
                .iter()
                .any(|line| line == "only-theirs: git has it, vigia does not"),
            "a one-directional sweep reads exactly like a two-directional one \
             when it comes back clean"
        );
    }

    #[test]
    fn a_missing_path_is_reported_in_both_directions_too() {
        let ours: BTreeSet<String> = ["a".to_owned(), "ours".to_owned()].into();
        let theirs: BTreeSet<String> = ["a".to_owned(), "theirs".to_owned()].into();
        assert_eq!(
            missing(&ours, &theirs, "git status"),
            vec![
                "ours: vigia lists it, git status does not".to_owned(),
                "theirs: git status lists it, vigia does not".to_owned(),
            ]
        );
    }

    #[test]
    fn identical_sides_report_no_disagreement() {
        // The positive control. Without it these two checks are only tested in
        // the direction that makes them fire, which is not the direction that
        // gets a check deleted for crying wolf.
        let rows = vec![
            Row {
                path: "a".to_owned(),
                added: Some(1),
                removed: Some(0),
            },
            Row {
                path: "b".to_owned(),
                added: None,
                removed: None,
            },
        ];
        assert!(
            disagreements(&rows, &rows).is_empty(),
            "two identical descriptions of one worktree disagree about nothing, \
             binary rows included"
        );
        let paths: BTreeSet<String> = ["a".to_owned(), "b".to_owned()].into();
        assert!(missing(&paths, &paths, "git status").is_empty());
    }

    /// A report over a run that measured almost nothing.
    ///
    /// Six frames and four samples, which is the shape the second real window
    /// actually had: a quiet session is the ordinary case here, not an edge, so
    /// it is the case the report has to survive.
    fn thin_report() -> Report {
        Report {
            tree: "vigia.a".to_owned(),
            window: Duration::from_secs(7200),
            step: Duration::from_secs(25),
            elapsed: Duration::from_secs(650),
            stopped_early: true,
            samples: (0..4)
                .map(|n| Sample {
                    at: Duration::from_secs(25 * n),
                    rss: 46_000_000 + n * 1024,
                    readout: Some(Duration::from_micros(1_600)),
                    files: 2,
                    tracked_diffs: 2,
                    tracked_spans: 2,
                    tracked_hunks: 3,
                    tracked_history: 4,
                    body: 20,
                })
                .collect(),
            frames: 6,
            ticks: 6,
            idle_waits: 26,
            failed: 0,
            last_error: None,
            hist: hist_of(&[1_600, 7_200, 9_890, 12_130, 53_010, 93_090]),
            worst: Vec::new(),
            watch: WatchStats {
                wakeups: 14_470,
                filtered: 14_435,
                ticks: 6,
            },
            frame: FrameStats::default(),
            highlight: HighlightStats::default(),
            history: HistoryStats::default(),
        }
    }

    #[test]
    fn a_run_that_measured_nothing_says_so_rather_than_printing_a_percentile() {
        let lines = thin_report().lines();
        let page = lines.join("\n");
        assert!(
            page.contains("too few for a p99"),
            "six frames must reach a reader as no percentile: {page}"
        );
        assert!(
            !page.contains("frame p50"),
            "and the percentile line must not also be there: {page}"
        );
        // The two figures that are exact at any count still have to arrive, or
        // refusing the percentile has cost the reader the answer as well as the
        // estimate.
        assert!(page.contains("at or over I9's 16ms budget: 2 of 6"));
        assert!(page.contains("max 93.09ms"));
        // And the cadence comes from the planned step, not from the samples that
        // happened to arrive before the run was stopped.
        assert!(
            page.contains("nominal 25s"),
            "a run stopped early still knows what it planned: {page}"
        );
    }

    #[test]
    fn a_report_with_frames_to_spare_does_quote_its_percentile() {
        // The positive control for the gate above: a report that suppressed the
        // percentile unconditionally would pass it.
        let mut report = thin_report();
        report.hist = hist_of(&[800; 200]);
        report.frames = 200;
        let page = report.lines().join("\n");
        assert!(page.contains("frame p50"), "{page}");
        assert!(!page.contains("too few for a p99"), "{page}");
    }

    #[test]
    fn quarter_medians_need_four_quarters_and_take_the_lower_middle() {
        assert_eq!(
            quarters(&[1, 2, 3]),
            None,
            "three samples are not four quarters"
        );
        assert_eq!(
            quarters(&[10, 12, 20, 22, 30, 32, 40, 42]),
            Some([10, 20, 30, 40]),
            "the lower of two middles, so every printed figure is a sample that \
             was actually taken"
        );
        assert_eq!(median(&[]), None);
    }
}
