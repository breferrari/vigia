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
pub const HISTORY_BUCKETS: usize = 8;

/// How much time one bucket covers.
pub const HISTORY_BUCKET: Duration =
    Duration::from_nanos(HISTORY_WINDOW.as_nanos() as u64 / HISTORY_BUCKETS as u64);

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

/// One path's churn, and when it last moved.
#[derive(Debug, Clone, Copy)]
struct Track {
    /// Oldest bucket first, so the array is drawn left to right as written.
    buckets: [u16; HISTORY_BUCKETS],
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
            buckets: [0; HISTORY_BUCKETS],
            tick,
        }
    }

    /// Slide `steps` buckets into the past, filling the newest end with zeroes.
    fn shift(&mut self, steps: usize) {
        if steps >= HISTORY_BUCKETS {
            self.buckets = [0; HISTORY_BUCKETS];
            return;
        }
        self.buckets.rotate_left(steps);
        self.buckets[HISTORY_BUCKETS - steps..].fill(0);
    }

    fn empty(&self) -> bool {
        self.buckets.iter().all(|&count| count == 0)
    }

    fn bump(&mut self) {
        let newest = &mut self.buckets[HISTORY_BUCKETS - 1];
        // Saturating rather than wrapping: a path written 65,536 times inside
        // one bucket is already at the top of the ramp, and wrapping would draw
        // the busiest file in the worktree as the quietest.
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
    /// When the newest bucket opened.
    opened: Instant,
    peak: u16,
    stats: HistoryStats,
}

impl History {
    /// An empty history, with its first bucket opening now.
    pub fn new() -> Self {
        Self::starting_at(Instant::now())
    }

    /// An empty history whose first bucket opened at `now`.
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
    pub fn churn(&self, path: &str) -> Option<[u16; HISTORY_BUCKETS]> {
        self.tracks.get(path).map(|track| track.buckets)
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
    /// moment it is the busiest thing it has ever been. Zero when nothing is
    /// tracked, which a caller must treat as "draw nothing" rather than dividing
    /// by it.
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
        let steps = usize::try_from(elapsed.as_nanos() / HISTORY_BUCKET.as_nanos())
            .unwrap_or(HISTORY_BUCKETS);
        if steps == 0 {
            return;
        }

        if steps >= HISTORY_BUCKETS {
            // The whole window has turned over, so nothing tracked can have a
            // sample left in it. Clearing beats shifting every track by more
            // buckets than it has, and it is the state a monitor left open
            // overnight wakes up in.
            self.stats.evicted_by_window += self.tracks.len() as u64;
            self.tracks.clear();
            self.opened = now;
            self.peak = 0;
            return;
        }

        // Advanced by whole buckets rather than set to `now`, so the boundaries
        // stay on a fixed grid and a burst of ticks inside one bucket cannot
        // walk it forward a fraction at a time.
        self.opened += HISTORY_BUCKET * steps as u32;

        let before = self.tracks.len();
        self.tracks.retain(|_, track| {
            track.shift(steps);
            !track.empty()
        });
        self.stats.evicted_by_window += (before - self.tracks.len()) as u64;
        self.repeak();
    }

    /// Drop the least recently changed path to make room for a new one.
    ///
    /// Least recently *changed* rather than least recently inserted: a path that
    /// keeps moving is the one a reader is watching, and evicting by age of
    /// arrival would throw it away in favour of something written once and
    /// forgotten.
    fn evict_one(&mut self) {
        let victim = self
            .tracks
            .iter()
            .min_by_key(|(path, track)| (track.tick, (*path).clone()))
            .map(|(path, _)| path.clone());
        if let Some(path) = victim {
            self.tracks.remove(&path);
            self.stats.evicted_by_cap += 1;
        }
    }

    fn repeak(&mut self) {
        self.peak = self
            .tracks
            .values()
            .flat_map(|track| track.buckets)
            .max()
            .unwrap_or(0);
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

    /// A path written more than `u16::MAX` times in one bucket is already at the
    /// top of the ramp; wrapping would draw the busiest file as the quietest.
    #[test]
    fn a_bucket_saturates_rather_than_wrapping() {
        let now = base();
        let mut history = History::starting_at(now);
        let mut track = Track::new(1);
        track.buckets[HISTORY_BUCKETS - 1] = u16::MAX;
        track.bump();
        assert_eq!(track.buckets[HISTORY_BUCKETS - 1], u16::MAX);

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
    /// inside one bucket must not walk the grid forward a fraction at a time,
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
