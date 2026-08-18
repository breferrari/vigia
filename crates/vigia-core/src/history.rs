//! Glanceability history: I10.
//!
//! > Churn history is bounded by a fixed window and a fixed cap on tracked
//! > paths, independent of how many files the session changed. A path that ages
//! > out of the window is dropped entirely.
//!
//! `SPEC.md` §5 makes shape and colour the whole differentiator, and three of
//! the things it asks for are the same question asked at different resolutions:
//! *when did this file last change, and how busy has it been.* A churn
//! sparkline, the recency gradient that dims a settled row, and the
//! `●` pulse all read from here.
//!
//! ## Why this cannot live in the frame path
//!
//! [`Frame`](crate::Frame) drops a diff the moment its path stops being
//! changed, and `FrameStats.evicted` exists to prove it: that is how I3 is
//! argued, because the map is then bounded by the current diff rather than by
//! everything ever edited. History needs the opposite. *What was hot thirty
//! seconds ago* is the entire question a sparkline answers, so it has to
//! survive a file settling, and a store that emptied when a file went quiet
//! would show nothing worth glancing at.
//!
//! Surviving is not the same as growing without limit, which is what I10 is
//! for. Two rules bound it, and both are tested:
//!
//! * **By window.** A path with no sample left inside [`HISTORY_WINDOW`] is
//!   dropped entirely, not merely drawn as empty.
//! * **By cap.** At [`HISTORY_PATHS`] the least recently changed path is
//!   evicted to make room. A bulk operation touching ten thousand files must
//!   not grow this past the cap, which is the case the gate in
//!   `tests/history.rs` actually drives rather than approximating.
//!
//! ## The clock is a tick, never a timer
//!
//! `SPEC.md` §5.1 asks the pulse to persist and decay, and the dimmed row to
//! fade as its last change ages. Taken literally against a wall clock, both need
//! a redraw to be *seen*, and **I1 forbids inventing a timer to get one**. That
//! is the same trap §10 already records for the highlight tail.
//!
//! So the window is real time and the *sampling* is not: [`History::record`] is
//! called once per coalesced tick, from the wake that was already going to
//! redraw. Nothing on screen changes without an event. A tree that has gone
//! quiet holds its last picture, which is what a monitor is supposed to do.
//!
//! The top rung of the ladder is deliberately **not** a duration for the same
//! reason. [`Recency::Pulse`] means *named by the most recent tick*, so it
//! cannot age into a lie while the loop is asleep, and it marks **every** path
//! in that tick rather than one. That is `SPEC.md` §11.2 B2's ruling arriving
//! from the other side: follow moves to the write that landed last, and the
//! pulse is what says the others moved too.
//!
//! ## One mechanism, not three
//!
//! §5.1 rules that the dimmed row and the pulse are one
//! mechanism, because specifying them separately would produce two decay clocks
//! that disagree on screen. They are three rungs of [`Recency`], read from one
//! store through one lookup, and [`Recency::Cold`] is not a fourth rule: it is
//! what I10's own eviction leaves behind.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How far back a churn sparkline reaches.
///
/// Two minutes. Long enough that a file which went quiet is still visibly
/// warmer than one nobody has touched, short enough that the whole strip turns
/// over while a reader is still working on the same thing. It is also the
/// gradient's only boundary: a path with a sample in this window is
/// [`Recency::Live`] and one without is [`Recency::Cold`], so there is one
/// number here rather than two that could disagree.
pub const HISTORY_WINDOW: Duration = Duration::from_secs(120);

/// Buckets a sparkline is drawn from, oldest first.
///
/// Eight, which is what the strip costs in columns. `SPEC.md` §11.1 makes a
/// sparkline a thing made of items, so at narrow widths it drops whole buckets
/// rather than being squeezed, and eight halves cleanly on the way down.
///
/// **This is what one element *draws*, and since
/// [#198](https://github.com/breferrari/vigia/issues/198) it is no longer what
/// the store *samples*.** The two were the same number for two phases, and the
/// reason was this docblock's own first sentence: a sampling rate chosen from one
/// element's column count. [`HISTORY_SAMPLES`] is the sampling rate now and
/// [`History::churn`] projects one onto the other, so this constant keeps its
/// exact meaning and its exact value.
pub const HISTORY_BUCKETS: usize = 12;

// Twelve since 2026-08-18 ([#161](https://github.com/breferrari/vigia/issues/161)),
// from eight, and the reason is the one [`GRAPH_COLUMNS`] was tuned against one
// element over. A bucket was fifteen seconds, which is coarse enough that a
// steady worktree drew only five and a half of the ramp's nine rungs and the
// strip read as a solid block rather than a shape. Ten seconds is finer without
// crossing into the scatter the band was reported for: measured over forty
// seeded series, a steady worktree goes from 8% of buckets empty to 17% and from
// 5.5 distinct heights to 6.5, which is more texture rather than more noise.
//
// **Twelve rather than fifteen**, which is where the band's own floor sits, for
// two reasons that are about this element rather than about the period. It has
// to halve cleanly, because `SPARK_RUNGS` is the narrowing ladder and fifteen
// does not; and the sparkline is per file and shares its row with a path, where
// the band has the pane to itself, so four more columns here cost something the
// band's do not.

/// How much time one **drawn** bucket covers.
pub const HISTORY_BUCKET: Duration =
    Duration::from_nanos(HISTORY_WINDOW.as_nanos() as u64 / HISTORY_BUCKETS as u64);

/// Columns of churn the band draws, however wide the pane is.
///
/// **A floor on the aggregation, which is what the band was missing.**
/// [`Churn::projected`] sums the samples under
/// each column, so a narrow band already shows the same total churn at a lower
/// resolution. What it did not do is *stop*: it clamped to the sample count, so
/// once the pane reached 120 columns one column was one second. A save is
/// instantaneous, so it filled a single column and left both neighbours empty,
/// and a wide pane bought time resolution nobody asked for instead of bigger
/// bars.
///
/// **Fifteen columns of eight seconds, and eight was tuned rather than chosen.**
/// Forty seeded series across three work patterns, measuring two things at once:
/// how much of the band is empty, and how many *distinct* heights survive.
/// Emptiness falls monotonically as columns coarsen, but the distinct heights
/// peak near four seconds and collapse by fifteen, so both ends lose the shape
/// for opposite reasons. A steady worktree draws 72% empty at one second, 25% at
/// eight, and 8% at fifteen with only five and a half of nine rungs left, which
/// is a solid block rather than a graph. Eight is the knee.
///
/// The window slides and its newest sample is always mid-accumulation, so the
/// newest column is systematically the shortest. That is a second reason to
/// coarsen: at one second a column it was up to **100%** incomplete and always
/// short until the second closed, and at eight it is at most 13%.
pub const GRAPH_COLUMNS: usize = 15;

/// How much time one **drawn** band column covers.
///
/// Named beside [`HISTORY_BUCKET`] rather than left in prose, which is where it
/// was: "eight seconds" appeared five times across the shell and the spec and
/// was computed nowhere.
pub const GRAPH_PERIOD: Duration =
    Duration::from_nanos(HISTORY_WINDOW.as_nanos() as u64 / GRAPH_COLUMNS as u64);

/// Samples the store keeps per path, oldest first.
///
/// **The resolution the store records at, which is not the resolution any one
/// element draws at** ([#198](https://github.com/breferrari/vigia/issues/198)).
/// It was [`HISTORY_BUCKETS`] until then, so one element's column count decided
/// what the whole store could ever answer, and a worktree-wide graph across a wide
/// pane had eight points to draw whatever glyph drew them.
///
/// **Not an I10 change.** That row bounds 256 paths and a 120-second window: the
/// path cap and the window are the invariant, and how finely the window is divided
/// sits underneath it. Nothing here changes how long a sample lives or how many
/// paths are kept.
///
/// **One hundred and twenty, so a sample is exactly one second** and exactly
/// fifteen of them make one drawn bucket. Both divisions are exact, which is what
/// keeps [`HISTORY_BUCKET`] an honest fifteen seconds rather than a rounding of
/// one: a projection that did not divide evenly would make some drawn columns
/// cover more time than others, and a sparkline compared down a list would be
/// comparing unequal windows.
///
/// The cost is `paths * samples` `u16`s, which at the cap is sixty kibibytes, and
/// a [`History::record`] walk fifteen times longer than it was. Both are measured
/// in the issue rather than asserted to be small.
///
/// **Public since [#158](https://github.com/breferrari/vigia/issues/158)**, which
/// is the second element and the one this resolution was raised for. It was kept
/// private while [`History::churn`] was the only reader, on the ground that
/// private is the reversible direction under semver and `pub` is not; the graph
/// draws the whole series across a whole pane, so it needs the length to project
/// against.
pub const HISTORY_SAMPLES: usize = 120;

/// How much time one **sample** covers, which is the grid the window rolls on.
const HISTORY_SAMPLE: Duration =
    Duration::from_nanos(HISTORY_WINDOW.as_nanos() as u64 / HISTORY_SAMPLES as u64);

/// How many samples one drawn bucket is the sum of.
///
/// **Exact, and asserted at compile time rather than by a test**, because a test
/// is the wrong instrument here twice over: the constants are private, so the
/// integration crate that would hold them cannot name them, and an inexact
/// division is not a behaviour to observe but a shape that must never build.
/// [`Track::drawn`] slices by this, so an inexact one would panic on the last
/// group rather than quietly dropping it, and the assertion below means neither
/// can happen.
///
/// The window's own tiling is asserted beside it for the same reason `roll`
/// needs it: `opened` advances by whole [`HISTORY_SAMPLE`]s, so a window that is
/// not a whole number of samples leaves a remainder no sample covers, which is
/// the argument `tests/history.rs` already makes one grid up for
/// [`HISTORY_BUCKET`].
const SAMPLES_PER_BUCKET: usize = HISTORY_SAMPLES / HISTORY_BUCKETS;

const _: () = {
    assert!(
        HISTORY_SAMPLES % GRAPH_COLUMNS == 0,
        "the samples do not divide into the band's columns, so a drawn column \n         would cover more time than its neighbours"
    );
    assert!(
        HISTORY_SAMPLES % HISTORY_BUCKETS == 0,
        "the samples do not divide into the drawn buckets, so a drawn column \
         would cover more time than its neighbours"
    );
    assert!(
        HISTORY_WINDOW.as_nanos() % HISTORY_SAMPLES as u128 == 0,
        "the samples do not tile the window, so a write can land in no sample"
    );
};

/// The most paths history will track at once.
///
/// The cap is the half of I10 that a window cannot provide: a bulk operation
/// puts ten thousand paths inside the window at the same instant, and without a
/// cap the store would be bounded by what the session did rather than by
/// anything fixed.
///
/// 256 is comfortably above any changed-file count a reader can look at (a
/// forty-row pane shows a few dozen) and small enough that the whole store is
/// tens of kilobytes against the ~26 MiB the soak measures. It is a bound, not
/// a tuning parameter: nothing is drawn from a path the reader cannot reach.
pub const HISTORY_PATHS: usize = 256;

/// How recently a path changed, as three rungs of one ladder.
///
/// Ordered by intensity rather than alphabetically, so the enum reads the way
/// the screen does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Recency {
    /// Named by the most recent tick. Drawn brightest, and the only rung that
    /// carries the `●` mark.
    ///
    /// Not a duration. See the module docs: a rung measured in seconds would
    /// have to age while the event loop is blocked, and being able to see that
    /// happen needs a redraw I1 forbids scheduling.
    Pulse,
    /// Changed inside [`HISTORY_WINDOW`], but not in the newest tick.
    Live,
    /// Nothing inside the window, so nothing is tracked for it at all.
    ///
    /// The ordinary state for a file that was edited before `vigia` started
    /// watching, which is the mockup's dimmed `Cargo.toml`.
    Cold,
}

/// What a [`History`] has done since it was created.
///
/// Cumulative, like [`FrameStats`](crate::FrameStats), so a test describes one
/// step by subtracting two readings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HistoryStats {
    /// Path samples taken, counting a path once per tick that named it.
    pub recorded: u64,
    /// Paths dropped to stay under [`HISTORY_PATHS`].
    ///
    /// The number that separates a bound which *held* from one that was never
    /// approached. A gate asserting the cap without asserting this passes
    /// against a store nothing ever filled.
    pub evicted_by_cap: u64,
    /// Paths dropped because nothing was left inside [`HISTORY_WINDOW`].
    pub evicted_by_window: u64,
}

/// Every tracked path's churn added together, oldest sample first.
///
/// **A newtype rather than a bare array, and the reason is `Default`.** Std only
/// implements it for arrays up to thirty-two long, and [`HISTORY_SAMPLES`] is
/// longer, so a bare array would take `Default` away from every type that holds
/// one. That reaches further than it looks: the shell's `View` derives it, and
/// most of the fixtures in its suite are struct-update literals that would all
/// have to name a field they do not care about.
///
/// It also gives the series somewhere to say what it is. A bare `[u32; N]` on a
/// struct is a length and nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Churn(pub [u32; HISTORY_SAMPLES]);

impl Default for Churn {
    fn default() -> Self {
        Self([0; HISTORY_SAMPLES])
    }
}

impl Churn {
    /// The series re-projected onto `width` columns, oldest first.
    ///
    /// **A projection re-projects; it does not drop items**, which is `heat_at`'s
    /// ruling in the shell and [`Track::drawn`]'s one crate over. Every column is
    /// the sum of the samples under it, so a narrow pane shows the same total
    /// churn at a lower resolution rather than a suffix of the window.
    ///
    /// The remainder is spread rather than dropped: with a width that does not
    /// divide [`HISTORY_SAMPLES`], the earlier columns take one extra sample
    /// each. Every sample lands in exactly one column, which is the property that
    /// matters, and no column is empty because the width is bounded by the caller
    /// to at most [`HISTORY_SAMPLES`].
    ///
    /// Saturating for [`Track::drawn`]'s reason: a column already at the top of
    /// the ramp must not wrap to the bottom of it.
    pub fn projected(&self, width: usize) -> Vec<u32> {
        if width == 0 {
            return Vec::new();
        }
        let width = width.min(HISTORY_SAMPLES);
        (0..width)
            .map(|column| {
                let from = column * HISTORY_SAMPLES / width;
                let to = (column + 1) * HISTORY_SAMPLES / width;
                self.0[from..to]
                    .iter()
                    .copied()
                    .fold(0u32, u32::saturating_add)
            })
            .collect()
    }
}

/// One path's churn, and when it last moved.
///
/// **`Clone` and not `Copy` since [#198](https://github.com/breferrari/vigia/issues/198)**,
/// which took this from twenty-four bytes to two hundred and forty-eight.
/// Nothing copies one today, and that is exactly why the trait comes off now: a
/// `Copy` of this size makes the next accidental by-value use invisible at the
/// call site and an order of magnitude more expensive than it reads.
#[derive(Debug, Clone)]
struct Track {
    /// Oldest sample first, so the array projects left to right as written.
    samples: [u16; HISTORY_SAMPLES],
    /// Ordinal of the last tick that named this path.
    ///
    /// A counter rather than an [`Instant`], because the only question asked of
    /// it is "was this the newest tick", and comparing ordinals cannot drift
    /// the way two clocks can.
    tick: u64,
}

impl Track {
    fn new(tick: u64) -> Self {
        Self {
            samples: [0; HISTORY_SAMPLES],
            tick,
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
    ///
    /// **Walked newest-first**, which is not a style choice: `samples` is
    /// oldest-first and a surviving track's writes are at the newest end, so a
    /// forward scan reads almost the whole array before it can return `false`
    /// for exactly the tracks that are kept. Reversed, the common case stops
    /// within a few reads and the genuinely empty case is identical.
    fn empty(&self) -> bool {
        self.samples.iter().rev().all(|&count| count == 0)
    }

    /// The samples summed into the buckets a sparkline draws, oldest first.
    ///
    /// **A projection re-projects; it does not drop items.** That is `heat_at`'s
    /// ruling in the shell, and it is the whole of why raising the sampling rate
    /// leaves the sparkline where it was: every drawn column is the sum of the
    /// [`SAMPLES_PER_BUCKET`] samples covering exactly the seconds it always
    /// covered, so the same writes land in the same column.
    ///
    /// Saturating, for [`Track::bump`]'s reason one level up: a column summing
    /// past `u16` is already at the top of the ramp, and wrapping would draw the
    /// busiest file in the worktree as the quietest.
    fn drawn(&self) -> [u16; HISTORY_BUCKETS] {
        // Sliced rather than zipped against `chunks`, which **truncates**: a
        // division that stopped being exact would silently drop the last group,
        // and the last group is the newest, so every fresh write would vanish
        // from the screen with nothing failing. Slicing panics instead, and the
        // `const` assertion beside `SAMPLES_PER_BUCKET` means it cannot.
        std::array::from_fn(|bucket| {
            self.samples[bucket * SAMPLES_PER_BUCKET..][..SAMPLES_PER_BUCKET]
                .iter()
                .copied()
                .fold(0, u16::saturating_add)
        })
    }

    fn bump(&mut self) {
        let newest = &mut self.samples[HISTORY_SAMPLES - 1];
        // Saturating rather than wrapping: a path written 65,536 times inside
        // one sample is already at the top of the ramp, and wrapping would draw
        // the busiest file in the worktree as the quietest. `Track::drawn`
        // saturates again when it sums a column, for the same reason one level
        // up.
        *newest = newest.saturating_add(1);
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
    ///
    /// Only those, because staging rewrites the index without changing a byte
    /// on disk, and letting it advance the counter would clear the pulse off
    /// the file the reader was watching for a reason they cannot see.
    tick: u64,
    /// When the newest **sample** opened, which is the grid the window rolls on.
    opened: Instant,
    peak: u16,
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
    ///
    /// Separate from [`History::new`] so a test can drive the window without
    /// sleeping through two minutes of it. `Instant` cannot be constructed from
    /// nothing, so the base has to come from somewhere real either way.
    pub fn starting_at(now: Instant) -> Self {
        Self {
            tracks: HashMap::new(),
            tick: 0,
            opened: now,
            peak: 0,
            worktree: Churn::default(),
            stats: HistoryStats::default(),
        }
    }

    /// Take one coalesced tick's worth of samples.
    ///
    /// Rolls the window forward to `now` first, so a sample lands in the bucket
    /// its wall-clock time belongs to rather than in whichever one was open when
    /// the last tick arrived.
    ///
    /// **An empty `paths` still rolls the window and still does not move the
    /// pulse.** That is the index-write case: staging is a real change and
    /// produces a real tick, because the index is the left-hand side of every
    /// diff drawn, but nothing on disk moved, so the file the reader is watching
    /// keeps its label.
    ///
    /// That is also **why there is no separate `expire`**, which the plan for
    /// [#38](https://github.com/breferrari/vigia/issues/38) named as a second
    /// public method. Aging the window is not a thing a caller ever wants on its
    /// own: it happens on a tick or it does not happen, because a tick is the
    /// only clock this type has (see the module docs on I1). A public `expire`
    /// would have been exactly `record` with no paths, and two entry points into
    /// one rule are two things a caller can get out of step. `vigia::run` calls
    /// this once per wake and nothing else, which is the whole contract.
    ///
    /// Called once per tick and never on a timer. See the module docs: the
    /// window is real time and the sampling is event-driven, which is what keeps
    /// I1 intact.
    ///
    /// # Cost
    ///
    /// A path already tracked is a hash lookup. A new one at the cap costs a
    /// scan for the least recently changed, which is bounded by
    /// [`HISTORY_PATHS`] and therefore constant rather than proportional to the
    /// session.
    pub fn record<'p>(&mut self, paths: impl IntoIterator<Item = &'p str>, now: Instant) {
        self.roll(now);

        let mut named = false;
        for path in paths {
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
                track.bump();
                continue;
            }

            if self.tracks.len() >= HISTORY_PATHS {
                self.evict_one();
            }
            let mut track = Track::new(self.tick);
            track.bump();
            self.tracks.insert(path.to_owned(), track);
        }

        self.repeak();
    }

    /// This path's buckets, oldest first, or `None` when nothing is tracked.
    ///
    /// Copied rather than borrowed. It is [`HISTORY_BUCKETS`] `u16`s, which is
    /// smaller than the reference on every target, and returning it by value is
    /// what lets a caller hold one while asking about the next path.
    ///
    /// **Projected rather than stored since
    /// [#198](https://github.com/breferrari/vigia/issues/198)**, which is what
    /// keeps the sentence above true: the store holds [`HISTORY_SAMPLES`] of them
    /// and this sums each drawn column's worth. Handing back the samples instead
    /// would have made the array larger than a reference on every target and
    /// pushed the projection out to every caller, where each would have had to
    /// agree about it.
    pub fn churn(&self, path: &str) -> Option<[u16; HISTORY_BUCKETS]> {
        self.tracks.get(path).map(Track::drawn)
    }

    /// Which rung of the recency ladder this path is on.
    pub fn recency(&self, path: &str) -> Recency {
        match self.tracks.get(path) {
            // `self.tick` is zero until something is recorded, and no track can
            // exist before then, so this never reads a pulse out of an empty
            // store.
            Some(track) if track.tick == self.tick => Recency::Pulse,
            Some(_) => Recency::Live,
            None => Recency::Cold,
        }
    }

    /// The largest bucket any tracked path holds.
    ///
    /// Rows share one scale rather than each being drawn against its own
    /// maximum, because the question a reader asks across a file list is which
    /// file is busiest, and per-file scaling draws every file at full height the
    /// moment it is the busiest thing it has ever been.
    ///
    /// **Zero when nothing is tracked, which is a scale a caller must not divide
    /// by.** It used to say the caller must treat it as "draw nothing", and that
    /// is no longer what the shell does: since
    /// [#78](https://github.com/breferrari/vigia/issues/78) an empty bucket draws
    /// a track, so a peak of zero means every bucket is empty and every one of
    /// them is still drawn. The constraint this states is arithmetic and belongs
    /// here; what to draw is the shell's and belongs in `SPEC.md` §5.1.
    pub fn peak(&self) -> u16 {
        self.peak
    }

    /// Paths currently tracked. Never more than [`HISTORY_PATHS`].
    ///
    /// This is the number I10 is a claim about: it follows the window and the
    /// cap, never the number of files the session has touched.
    pub fn tracked(&self) -> usize {
        self.tracks.len()
    }

    /// Counters for what this history has done.
    pub fn stats(&self) -> HistoryStats {
        self.stats
    }

    /// Advance the window to `now`, dropping whatever fell out of it.
    fn roll(&mut self, now: Instant) {
        let elapsed = now.saturating_duration_since(self.opened);
        // Saturating into `usize` before the comparison below, so an instant far
        // in the future cannot overflow the multiplication that moves `opened`.
        let steps = usize::try_from(elapsed.as_nanos() / HISTORY_SAMPLE.as_nanos())
            .unwrap_or(HISTORY_SAMPLES);
        if steps == 0 {
            return;
        }

        if steps >= HISTORY_SAMPLES {
            // The whole window has turned over, so nothing tracked can have a
            // sample left in it. Clearing beats shifting every track by more
            // samples than it has, and it is the state a monitor left open
            // overnight wakes up in.
            self.stats.evicted_by_window += self.tracks.len() as u64;
            self.tracks.clear();
            self.opened = now;
            self.peak = 0;
            return;
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
        // **No repeak here**, deliberately: `record` is this function's only
        // caller and repeaks unconditionally after it returns, so a second full
        // projection of every track would be pure duplicate work. That was
        // survivable while a track held eight samples and is a quarter of the
        // tick's cost now that it holds a hundred and twenty.
    }

    /// Drop the least recently changed path to make room for a new one.
    ///
    /// Least recently *changed* rather than least recently inserted: a path that
    /// keeps moving is the one a reader is watching, and evicting by age of
    /// arrival would throw it away in favour of something written once and
    /// forgotten.
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

    /// Every tracked path's churn added together, oldest sample first.
    ///
    /// **The worktree's own series, which is what `SPEC.md` §5.3 calls the
    /// worktree churn graph** ([#158](https://github.com/breferrari/vigia/issues/158)).
    /// It invents nothing: it is arithmetic over state I10 already bounds, so it
    /// needs no wake, no write and no clock, and it is drawn on frames that were
    /// going to happen anyway.
    ///
    /// **Maintained rather than computed on demand, and it is free**, because
    /// [`History::repeak`] already walks every sample of every track on every
    /// [`History::record`]. Summing in that same pass costs one add per element
    /// and the frame path pays nothing at all: a caller asking for this is a
    /// field read.
    ///
    /// `u32` because the sum is over paths as well as time. At the cap that is
    /// 256 paths of `u16`, which overflows `u16` and cannot overflow `u32`.
    pub fn worktree_churn(&self) -> Churn {
        self.worktree
    }

    /// Recompute the busiest drawn bucket and the worktree series, in one walk.
    ///
    /// **Two results from one pass, which is why the series is free.** This
    /// already had to touch every sample of every track to find the peak, so
    /// [`History::worktree_churn`]'s sum rides along at one add per element
    /// rather than costing a walk of its own. Splitting them would double the
    /// most expensive thing a tick does.
    ///
    /// The peak is over the **projection** rather than the raw samples, which is
    /// the half of [#198](https://github.com/breferrari/vigia/issues/198) that
    /// would have moved the sparkline if it were got wrong: heights are scaled
    /// against it, so a denominator measured one sample at a time would be
    /// smaller than the columns it divides and every bar on screen would top
    /// out.
    fn repeak(&mut self) {
        let mut peak = 0u16;
        let mut worktree = [0u32; HISTORY_SAMPLES];
        for track in self.tracks.values() {
            for (total, &count) in worktree.iter_mut().zip(track.samples.iter()) {
                *total += u32::from(count);
            }
            peak = peak.max(track.drawn().into_iter().max().unwrap_or(0));
        }
        self.peak = peak;
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
    //!
    //! Every one of these drives the clock by handing [`History::record`] an
    //! instant rather than by sleeping. That is not only for speed: the window
    //! is two minutes long and the cap needs a bulk write, so a suite that
    //! waited for real time could not gate either of them at all.

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
        assert_eq!(history.peak(), 0);
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

    #[test]
    fn peak_is_the_largest_bucket_across_paths_so_rows_share_a_scale() {
        let now = base();
        let mut history = History::starting_at(now);
        history.record(["a"], now);
        history.record(["a"], now);
        history.record(["b"], now);

        assert_eq!(history.peak(), 2);
        assert_eq!(history.churn("b").unwrap()[HISTORY_BUCKETS - 1], 1);
    }

    /// A path written more than `u16::MAX` times in one sample is already at the
    /// top of the ramp; wrapping would draw the busiest file as the quietest.
    ///
    /// **Both halves, since #198 gave the projection its own saturating add.** A
    /// sample that saturates and a drawn column that sums fifteen of them are two
    /// places the same wrap could happen, and the second is the newer one.
    #[test]
    fn a_bucket_saturates_rather_than_wrapping() {
        let now = base();
        let mut history = History::starting_at(now);
        let mut track = Track::new(1);
        track.samples[HISTORY_SAMPLES - 1] = u16::MAX;
        track.bump();
        assert_eq!(track.samples[HISTORY_SAMPLES - 1], u16::MAX);
        track.samples[HISTORY_SAMPLES - 2] = 9;
        assert_eq!(
            track.drawn()[HISTORY_BUCKETS - 1],
            u16::MAX,
            "a drawn column summing past u16 wrapped instead of topping out"
        );

        history.record(["a"], now);
        assert!(history.peak() > 0);
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
        assert_eq!(history.peak(), 0);
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
