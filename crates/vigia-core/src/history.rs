//! Glanceability history: I10.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How far back a churn sparkline reaches.
pub const HISTORY_WINDOW: Duration = Duration::from_secs(120);

/// The resolution a sparkline is projected **from**, oldest bucket first.
pub const HISTORY_BUCKETS: usize = 24;

// Eight, twelve and twenty-four were each argued from a different thing. Eight
// was one element's column count.
// Twelve was a period: a fifteen-second bucket is coarse enough that a steady
// worktree drew five and a half of the ramp's nine rungs, which is a block rather
// than a shape, and ten seconds took it to 6.5 without crossing into scatter.
// Twenty-four is neither, because a *rung* has no single period: it is the widest
// division the ladder can halve twice through twelve.

/// Source buckets one **drawn** bucket may cover, finest first.
pub const SPARK_GROUPS: [usize; 3] = [1, 2, 4];

const _: () = {
    let mut at = 0;
    while at < SPARK_GROUPS.len() {
        assert!(
            HISTORY_BUCKETS % SPARK_GROUPS[at] == 0,
            "a sparkline grouping does not divide the source resolution, so a \
             drawn bucket would cover less time than its neighbours"
        );
        // **Each grouping divides the next, which is stronger than ascending and
        // is what [`History::repeak`]'s ordering proof actually rests on.** That
        // proof says a coarser rung's figure cannot be smaller because coarsening
        // *merges* groups, so the kept sum is shared and the non-empty count can
        // only fall. Merging is what needs this: chunks of four are unions of
        // chunks of two, so every group at one rung lies inside one group at the
        // next.
        assert!(
            at == 0 || SPARK_GROUPS[at] % SPARK_GROUPS[at - 1] == 0,
            "a sparkline grouping does not divide into the next, so a coarser \
             rung is not a merge of a finer one and their figures need not come \
             out in order"
        );
        at += 1;
    }
};

/// How much time one **source** bucket covers, which is the finest a rung draws.
pub const HISTORY_BUCKET: Duration =
    Duration::from_nanos(HISTORY_WINDOW.as_nanos() as u64 / HISTORY_BUCKETS as u64);

// **`GRAPH_COLUMNS` and `GRAPH_PERIOD` were here and are retired**
// ([#232](https://github.com/breferrari/vigia/issues/232)). They fixed the
// band's period at fifteen columns of eight seconds, tuned over forty seeded
// series. That diagnosed the right defect, a save drawing a
// one-column hairline between two blanks, and reached for the wrong fix: the
// answer to the
// same defect on the same shape of signal is the **axis**, and with a
// floor under it a narrow column is a spike rather than a mark in a void. The
// band draws one value per sub-column now, so its period is a property of the
// pane and there is no constant to name.

/// Samples the store keeps per path, oldest first.
pub const HISTORY_SAMPLES: usize = 120;

/// How much time one **sample** covers, which is the grid the window rolls on.
pub const HISTORY_SAMPLE: Duration =
    Duration::from_nanos(HISTORY_WINDOW.as_nanos() as u64 / HISTORY_SAMPLES as u64);

/// Samples at the newest end of a track that keep [`Recency::Pulse`] on it.
pub const PULSE_SAMPLES: usize = 2;

// **A slice of the newest samples has to fit inside the window it slices**, and
// `recency` indexes with a subtraction: past `HISTORY_SAMPLES` that underflows and
// panics on the frame path, where a monitor that panics is the worst failure this
// product has. A `const` block is the instrument this repository already reaches
// for when a claim no test can fail is a wish, and it stops the build rather than
// a suite.
const _: () = assert!(
    PULSE_SAMPLES >= 1 && PULSE_SAMPLES <= HISTORY_SAMPLES,
    "PULSE_SAMPLES must name at least one sample and no more than the window holds"
);

/// How many samples one source bucket is the sum of.
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
pub const HISTORY_PATHS: usize = 256;

/// How recently a path changed, as three rungs of one ladder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Recency {
    /// Named by the most recent tick **and** holding ink in the newest sample.
    /// Drawn brightest.
    Pulse,
    /// Changed inside [`HISTORY_WINDOW`], but not in the newest tick.
    Live,
    /// Nothing inside the window, so nothing is tracked for it at all.
    Cold,
}

/// What a [`History`] has done since it was created.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HistoryStats {
    /// Path samples taken, counting a path once per tick that named it.
    pub recorded: u64,
    /// Paths dropped to stay under [`HISTORY_PATHS`].
    pub evicted_by_cap: u64,
    /// Paths dropped because nothing was left inside [`HISTORY_WINDOW`].
    pub evicted_by_window: u64,
    /// Projections walked, which is the work the ageing clock makes worth
    /// counting.
    pub repeaks: u64,
}

/// Every tracked path's churn added together, oldest sample first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Churn(pub [u32; HISTORY_SAMPLES]);

/// How far a write's weight is spread when the series is read as a **level**.
pub const HISTORY_LEVEL: Duration = Duration::from_secs(6);

/// [`HISTORY_LEVEL`] in samples, which is what the filter actually steps in.
const LEVEL_SAMPLES: f64 = HISTORY_LEVEL.as_nanos() as f64 / HISTORY_SAMPLE.as_nanos() as f64;

/// Sum a retained series into the source buckets a sparkline is drawn from.
fn bucketed(samples: &[u32; HISTORY_SAMPLES]) -> [u32; HISTORY_BUCKETS] {
    std::array::from_fn(|bucket| {
        samples[bucket * SAMPLES_PER_BUCKET..][..SAMPLES_PER_BUCKET]
            .iter()
            .copied()
            .fold(0, u32::saturating_add)
    })
}

/// Read a retained series as a level rather than as the events that made it.
fn levelled(samples: &[u32; HISTORY_SAMPLES]) -> [u32; HISTORY_SAMPLES] {
    let decay = (-1.0 / LEVEL_SAMPLES).exp();

    // Two passes over the samples, and two more over a flat series of ones. The
    // second pair is the **weight actually available** at each position, and
    // dividing by it is what makes this a weighted average rather than a sum.
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
    pub fn levels(&self, width: usize) -> Vec<u32> {
        Churn(levelled(&self.0)).projected(width)
    }

    /// What [`Self::levels`] at this width is measured against.
    pub fn scale_at(&self, width: usize) -> u32 {
        let levelled = levelled(&self.0);
        let mut busy: Vec<u32> = levelled.iter().copied().filter(|v| *v > 0).collect();
        let Some(cut) = outlier_cut(&mut busy) else {
            return 0;
        };
        let kept = Churn(std::array::from_fn(|at| {
            let sample = levelled[at];
            if u64::from(sample) <= cut { sample } else { 0 }
        }));
        let (sum, count) = kept
            .projected(width)
            .into_iter()
            .map(u64::from)
            .filter(|column| *column > 0)
            .fold((0u64, 0u64), |(sum, count), column| {
                (sum + column, count + 1)
            });
        scale_from(sum, count)
    }
}

/// What a churn height is measured against: above the ordinary write, not at the
/// largest one, and not dragged up by a burst either.
pub fn scale_of(values: impl Iterator<Item = u32>) -> u32 {
    let mut busy: Vec<u32> = values.filter(|value| *value > 0).collect();
    scale_of_busy(&mut busy)
}

/// How many times the median a value may be before it stops setting the scale.
const SCALE_OUTLIER: u64 = 10;

/// [`scale_of`] over a scratch of the **non-empty** values, which it reorders.
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
fn outlier_cut(busy: &mut [u32]) -> Option<u64> {
    if busy.is_empty() {
        return None;
    }
    let mid = busy.len() / 2;
    let (_, median, _) = busy.select_nth_unstable(mid);
    Some(u64::from(*median) * SCALE_OUTLIER)
}

/// Thirteen tenths of the mean of what the cut kept.
fn scale_from(sum: u64, kept: u64) -> u32 {
    if kept == 0 {
        // Nothing to average. The cut never causes it, because `outlier_cut`
        // answers `None` for an empty population and every non-empty one keeps
        // its own median; what does is a caller with no columns to measure, which
        // is [`Churn::scale_at`] at width zero. Zero is what every caller reads
        // as "no scale yet".
        return 0;
    }
    // Thirteen tenths: above the mean, so an ordinary write does not sit at the
    // ceiling, and close enough to it that an ordinary write is still legible as
    // a shape rather than as a stub.
    u32::try_from(sum * 13 / (kept * 10)).unwrap_or(u32::MAX)
}

/// One path's churn, and when it last moved.
#[derive(Debug, Clone)]
struct Track {
    /// Oldest sample first, so the array projects left to right as written.
    samples: [u32; HISTORY_SAMPLES],
    /// Ordinal of the last tick that named this path.
    tick: u64,
    /// Bytes this path held when it was last weighed, if it ever has been.
    bytes: Option<u64>,
}

impl Track {
    /// Whether this track was named by the burst numbered `tick`.
    fn named_by(&self, tick: u64) -> bool {
        self.tick == tick
    }

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
    fn empty(&self) -> bool {
        self.samples.iter().rev().all(|&count| count == 0)
    }

    /// The samples summed into the source buckets a sparkline is drawn from,
    /// oldest first.
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
    fn levelled(&self) -> [u32; HISTORY_BUCKETS] {
        bucketed(&levelled(&self.samples))
    }

    /// Add this write's weight to the newest sample.
    fn bump(&mut self, weight: u32) {
        let newest = &mut self.samples[HISTORY_SAMPLES - 1];
        *newest = newest.saturating_add(weight.max(1));
    }

    /// What this write weighs, and remember the size it was weighed against.
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
    tick: u64,
    /// When the newest **sample** opened, which is the grid the window rolls on.
    opened: Instant,
    /// What a drawn bucket's height is divided by, one figure per
    /// [`SPARK_GROUPS`] entry. See [`scale_of`].
    scales: [u32; SPARK_GROUPS.len()],
    /// Every tracked path's levelled **source** buckets, one path's contiguous,
    /// held between ticks so [`Self::repeak`] allocates nothing.
    scratch: Vec<u32>,
    /// The non-empty members of [`Self::scratch`], which is the population the
    /// median is taken over.
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
    pub fn record<'p>(&mut self, paths: impl IntoIterator<Item = &'p str>, now: Instant) {
        self.record_sized(paths.into_iter().map(|path| (path, None)), now);
    }

    /// [`History::record`], with what each written path now holds on disk.
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

        // **Skipped when nothing moved and nothing was written**, which is not
        // an optimisation looking for a problem: this call sits on the shell
        // loop's *timeout* arm, which also fires every `STEP_REPEAT` while
        // a scrollbar button is held. At 50ms a held button drives twenty
        // timeouts a second and nineteen of them cross no sample boundary, so
        // without this guard each one pays the walk priced at **150.1µs p50** in
        // [`Self::repeak`]'s own docblock, for an answer that
        // cannot have changed.
        if rolled > 0 || named {
            self.stats.repeaks += 1;
            self.repeak();
        }
    }

    /// This path's buckets, oldest first, or `None` when nothing is tracked.
    pub fn churn(&self, path: &str) -> Option<[u32; HISTORY_BUCKETS]> {
        self.tracks.get(path).map(Track::drawn)
    }

    /// This path's churn read as a **level** rather than as the writes that made
    /// it, in the same source buckets [`History::churn`] returns.
    pub fn level(&self, path: &str) -> Option<[u32; HISTORY_BUCKETS]> {
        self.tracks.get(path).map(Track::levelled)
    }

    /// Whether the newest burst named this path, which is what the `●` marks.
    pub fn newest(&self, path: &str) -> bool {
        // `self.tick` is zero until something is recorded and no track can exist
        // before then, so this never reads a mark out of an empty store.
        self.tracks
            .get(path)
            .is_some_and(|track| track.named_by(self.tick))
    }

    /// Which rung of the recency ladder this path is on.
    pub fn recency(&self, path: &str) -> Recency {
        match self.tracks.get(path) {
            // `self.tick` is zero until something is recorded and no track can
            // exist before then, so this never reads a pulse out of an empty
            // store. `Track::bump` floors a write at one, so a non-zero sample in
            // the newest [`PULSE_SAMPLES`] is exactly "written within the last
            // sample or the one before it".
            Some(track)
                if track.named_by(self.tick)
                    && track.samples[HISTORY_SAMPLES - PULSE_SAMPLES..]
                        .iter()
                        .any(|&count| count > 0) =>
            {
                Recency::Pulse
            }
            Some(_) => Recency::Live,
            None => Recency::Cold,
        }
    }

    /// What a **source** bucket's height is divided by, across every tracked path.
    pub fn scale(&self) -> u32 {
        self.scales[0]
    }

    /// Every figure [`SPARK_GROUPS`] names, in that order.
    pub fn scales(&self) -> [u32; SPARK_GROUPS.len()] {
        self.scales
    }

    /// Paths currently tracked. Never more than [`HISTORY_PATHS`].
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
    /// change.
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
            // Zeroing `scales` and leaving `worktree` alone is harmless only
            // while the caller repeaks unconditionally. It does not, and a
            // branch that clears one of two derived fields is one edit from
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
        // twenty. (Repeaking *unconditionally* is what this function's step
        // count lets the caller skip.)
        steps
    }

    /// Drop the least recently changed path to make room for a new one.
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
    pub fn ages_in(&self, now: Instant) -> Option<Duration> {
        if self.tracks.is_empty() {
            return None;
        }
        Some((self.opened + HISTORY_SAMPLE).saturating_duration_since(now))
    }

    /// Every tracked path's churn added together, oldest sample first.
    pub fn worktree_churn(&self) -> Churn {
        self.worktree
    }

    /// Recompute the busiest source bucket and the worktree series, in one walk.
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
        // **Collected rather than streamed.** Walking the buckets through an
        // iterator means nothing has to hold them, and the rule takes a median,
        // which no running pair can answer.
        // [`Self::scratch`] is what keeps that from costing an allocation a tick.
        // **Measured rather than assumed, since this is the frame path, and
        // re-measured when this body changes** rather than left describing the
        // code it was taken against. At the full
        // 256-path cap over a populated window this walk costs **150.1µs p50 and
        // 154.2µs p99**, from 147.0 and 150.7 with the cut disabled and the rest
        // of the body unchanged, measured interleaved in one process so a loaded
        // machine moves both arms. `levelled` is two O(n) passes over 120 samples per track rather
        // than the O(n·k) convolution its shape suggests, which is why the gather
        // and the median are not what costs here.
        self.scratch.clear();
        self.scratch
            .reserve(self.tracks.len().saturating_mul(HISTORY_BUCKETS));
        for track in self.tracks.values() {
            self.scratch.extend_from_slice(&track.levelled());
        }

        // **What is outlying is decided once, at the source resolution, and then
        // every rung sums one kept series.** That ordering is not a detail: it is
        // what makes a coarser rung's figure provably no smaller than a finer
        // rung's, which is what `SPEC.md` §11.1 needs for a width rung to be a
        // change of resolution rather than of height.
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
                    // **Summed in `u64`, and that is load bearing rather than
                    // tidy.** A `u32` fold saturates, and four kept buckets can
                    // saturate as one chunk where the same four do not one at a
                    // time: the total would then be smaller at a coarse rung than
                    // at a fine one, the shared sum in the proof above would stop
                    // being shared, and the ordering would fail in the exact
                    // direction this row exists to fix. The accumulator was
                    // already `u64`; the widening was one line too late.
                    let total: u64 = chunk
                        .iter()
                        .map(|bucket| u64::from(*bucket))
                        .filter(|bucket| *bucket <= cut)
                        .sum();
                    if total > 0 {
                        part.0 += total;
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
        // against one bucket.** `scale < busiest` is a true consequence of a
        // mean-based scale while the buckets are spiky and stops being one once
        // levelling makes them smooth: a smooth series has few outliers, so a
        // mean is
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
