use std::time::Duration;

/// What one refresh cost, and what it produced.
///
/// The two phases are separated because they answer different invariants:
/// `enumerate` is the first-paint path (I4, I7), `diff` is the steady-state
/// path (I9).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameTiming {
    /// Time to walk the working tree and name every changed path.
    pub enumerate: Duration,
    /// Time to turn those paths into hunks.
    pub diff: Duration,
    /// Changed paths found.
    pub files: usize,
    /// Hunks produced across all files.
    pub hunks: usize,
    /// Lines added across all files.
    pub added: u32,
    /// Lines removed across all files.
    pub removed: u32,
    /// Bytes of content read, from the object database and from disk.
    pub bytes: u64,
}

impl FrameTiming {
    /// Wall time for the whole refresh.
    pub fn total(&self) -> Duration {
        self.enumerate + self.diff
    }
}

/// A fixed-capacity collection of frame durations for percentile reporting.
///
/// Bounded on purpose. I3 forbids unbounded growth over a multi-day run, and a
/// timing buffer that grows per frame would be the first thing to violate it.
#[derive(Debug, Clone)]
pub struct Samples {
    values: Vec<Duration>,
    next: usize,
    filled: bool,
}

impl Samples {
    /// Create a buffer retaining the most recent `capacity` samples.
    ///
    /// # Panics
    ///
    /// Panics if `capacity` is zero.
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "Samples needs room for at least one value");
        Self {
            values: vec![Duration::ZERO; capacity],
            next: 0,
            filled: false,
        }
    }

    /// Record one sample, evicting the oldest once full.
    pub fn push(&mut self, value: Duration) {
        self.values[self.next] = value;
        self.next += 1;
        if self.next == self.values.len() {
            self.next = 0;
            self.filled = true;
        }
    }

    /// How many samples are currently retained.
    pub fn len(&self) -> usize {
        if self.filled {
            self.values.len()
        } else {
            self.next
        }
    }

    /// Whether no samples have been recorded.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The `q`th percentile, with `q` in `0.0..=1.0`, or `None` when empty.
    ///
    /// Nearest-rank, which is the honest choice for a latency budget: it
    /// returns a duration that actually occurred rather than an interpolation
    /// between two that did.
    pub fn percentile(&self, q: f64) -> Option<Duration> {
        let len = self.len();
        if len == 0 {
            return None;
        }
        let mut sorted: Vec<Duration> = self.values[..len].to_vec();
        sorted.sort_unstable();
        let rank = (q.clamp(0.0, 1.0) * len as f64).ceil() as usize;
        Some(sorted[rank.saturating_sub(1).min(len - 1)])
    }

    /// The largest retained sample.
    pub fn max(&self) -> Option<Duration> {
        self.values[..self.len()].iter().copied().max()
    }

    /// Every retained sample added together.
    ///
    /// **What the budget gates attribute an overshoot with** (`SPEC.md` §7,
    /// [#178](https://github.com/breferrari/vigia/issues/178)): a round's wall clock
    /// less the same round's thread CPU time is the time the process spent not
    /// running, and a sum is the only form in which that comparison survives
    /// Windows' scheduler-tick granularity. Saturating, so a ring of long samples
    /// cannot overflow the answer.
    pub fn total(&self) -> Duration {
        self.values[..self.len()]
            .iter()
            .fold(Duration::ZERO, |sum, each| sum.saturating_add(*each))
    }

    /// How much of this round sat **above** `budget`, summed over the samples
    /// that exceeded it.
    ///
    /// The companion to [`Self::total`], and it exists because attributing a
    /// breach to the host needs both sides of the comparison in the same units.
    /// A round's off-CPU time is a sum over every sample; the thing it has to
    /// explain is therefore also a sum, not one sample's excess. Comparing the
    /// sum against a single percentile's overshoot drifts toward acquittal as
    /// the sample count rises, because ordinary per-frame scheduling noise
    /// accumulates while the per-frame overshoot does not.
    ///
    /// Samples at or under `budget` contribute nothing, so a round that is
    /// entirely inside budget returns zero and a single slow frame contributes
    /// exactly its own excess. Saturating for the same reason [`Self::total`]
    /// is.
    pub fn excess_over(&self, budget: Duration) -> Duration {
        self.values[..self.len()]
            .iter()
            .fold(Duration::ZERO, |sum, each| {
                sum.saturating_add(each.saturating_sub(budget))
            })
    }
}
