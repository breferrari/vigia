//! Drawing a [`View`] into a buffer, and nothing else.
//!
//! Pure: same view, same theme, same area, same cells. That is what makes the
//! snapshot suite worth anything, and it is why the frame path, the scroll
//! arithmetic and the terminal all live somewhere else.
//!
//! **No borders, no boxes.** `btop` frames everything, and `btop` has the whole
//! screen. Half a laptop screen beside an agent is the case I6 names, and a box
//! spends two of forty columns and two of twenty-four rows on decoration. Every
//! cell here goes to the diff or to the chrome: one line at the top, and one at
//! the bottom that takes a second only when forty columns cannot hold it.
//!
//! Content is written cell by cell rather than through the widget set. A
//! `Paragraph` would wrap, and wrapping is the one thing a monitor must not do:
//! a wrapped diff line moves every line below it, so the shape of the screen
//! stops meaning anything. Lines are clipped instead.
//!
//! ## I6, which is the whole of the layout
//!
//! `SPEC.md` §11.1 states the rule this module implements: **a thing made of
//! items breaks, a thing made of characters marks its edge, and content is
//! neither.**
//!
//! The hint bar is the only thing on screen made of items, so it is the only
//! thing that breaks — onto [`Footer`]'s second line, and then by dropping whole
//! rungs of [`HINT_RUNGS`]. Everything else is one token and says which end it
//! lost: [`ELIDED`] on the left for a file path, whose tail names the file, and
//! [`CONTINUES`] on the right for everything else, content included.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span as TextSpan;
use vigia_core::{Class, HISTORY_BUCKETS, LineKind, Recency, Span};

use crate::theme::Theme;
use crate::view::{HEAT_BUCKETS, HeatBucket, Row, View};

/// Columns a tab advances to the next multiple of.
///
/// Four, and not configurable: `SPEC.md` names no setting for it, and a monitor
/// that has to be told how to draw a tab has already lost. Expanding matters
/// more than the number does, because a raw `\t` written into a terminal cell
/// renders as nothing and silently misaligns everything after it.
const TAB_STOP: usize = 4;

/// Stands in for a character that cannot be drawn.
///
/// Chosen for reach rather than beauty: U+00B7 is in Latin-1 and in CP437, so it
/// survives the legacy Windows console that `SPEC.md` §10 leaves open, and it
/// does not read as file content the way `.` or `?` would.
const UNPRINTABLE: char = '·';

/// Shown where a path had to lose its head to fit.
const ELIDED: char = '…';

/// Shown where anything else ran past the right edge.
///
/// The two marks are two directions and never overlap: [`ELIDED`] on the left
/// says the beginning is gone, this one on the right says it continues. A
/// reader never has to work out which end they are missing.
const CONTINUES: &str = "›";

/// The footer's left-hand side when there is nothing wrong, widest rung first.
///
/// **Each rung is the one above it minus whole hints**, and nothing is reworded
/// on the way down: a bar that shortened `jk scroll` to `jk scr` is the
/// truncated-to-useless shape I6 forbids, and one that invented a shorter
/// wording would teach a second dialect at exactly the width where the reader
/// has least to go on. `tests/legibility.rs` gates both properties over the
/// rungs it observes by rendering, so this table cannot drift from what ships.
///
/// The drop order is `SPEC.md` §11.1's ruling. `jk scroll` goes first and
/// `f follow` is last standing: `q` and `jk` are pager reflexes and four keys
/// reach quit, while `f` is the one nobody would guess and the only one that
/// restores a state a reader can lose without noticing. It only fires below
/// twenty-nine columns, because above that [`Footer`] gives the bar a line of
/// its own rather than shortening it.
const HINT_RUNGS: [&str; 4] = [
    "q quit · f follow · jk scroll",
    "q quit · f follow",
    "f follow",
    "",
];

/// What joins two hints.
///
/// Exported because `tests/legibility.rs` splits the rendered bar on it to check
/// that every hint on screen is a whole one. A test that restated the separator
/// as its own literal would be a second implementation of the parse, agreeing
/// with itself while disagreeing with the screen. The ladder is deliberately
/// **not** exported for the same reason inverted: a test comparing the rung
/// table against itself proves nothing, so the rungs are observed by rendering.
pub const HINT_SEPARATOR: &str = " · ";

/// A churn bucket's height, emptiest first.
///
/// The eighth-blocks every sparkline in every terminal is drawn from. They are
/// outside CP437, like the `▶` the footer has carried since I5, so the legacy
/// Windows console `SPEC.md` §10 leaves open degrades on both together rather
/// than on this alone.
const SPARK_RAMP: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// A bucket nothing happened in.
///
/// A space rather than the lowest block. `▁` would say "a little happened",
/// which is a different claim from "nothing did", and over eight buckets the
/// difference is what tells a settling file from a busy one.
const SPARK_EMPTY: char = ' ';

/// How many buckets a sparkline may show, widest rung first.
///
/// A sparkline is **a thing made of items**, so `SPEC.md` §11.1 makes it break
/// rather than mark an edge: it drops whole buckets, oldest first, and never
/// draws a partial one. Halving keeps the remaining strip readable as a shape,
/// where shaving one bucket at a time would leave widths where the picture is
/// neither the full window nor an obvious fraction of it.
const SPARK_RUNGS: [usize; 3] = [HISTORY_BUCKETS, HISTORY_BUCKETS / 2, 0];

/// The pulse, widest rung first.
///
/// `SPEC.md` §5.1 draws this as a persisting label with a dot rather than a
/// flash. The mark survives narrowing on its own, for the reason `f follow` is
/// the last hint standing: it is the one signal on the row that cannot be
/// recovered from anything else on screen, and one column is what it costs.
const PULSE_RUNGS: [&str; 3] = ["● just changed", "●", ""];

/// One slice of a file, whatever it holds.
///
/// A solid block, not a ramp. The sparkline two columns away already encodes
/// magnitude as height, and a second magnitude encoding beside it would be a
/// second dialect for one fact. Here the block is a position and the **colour**
/// is the meaning, which is exactly what `assets/preview.svg` draws: twelve
/// rects of equal height differing only in fill.
const HEAT_BLOCK: char = '█';

/// How many slices the heat strip may show, widest rung first.
///
/// **A projection re-projects; it does not drop items**, and that is the third
/// case of `SPEC.md` §11.1's rule rather than an instance of the first. The hint
/// bar and the sparkline are lists, so dropping an item shows less. A heat strip
/// that dropped its last six buckets would show the first half of the file
/// *drawn as the whole of it*, and a reader would read an untouched tail. So a
/// narrower rung sums adjacent buckets and classifies the sums: less resolution,
/// still the whole file.
///
/// Halves, so the sum is exact and every drawn bucket covers the same span.
const HEAT_RUNGS: [usize; 3] = [HEAT_BUCKETS, HEAT_BUCKETS / 2, 0];

/// Columns a path keeps before any glance element is allowed to exist.
///
/// A heading whose path has been elided past this is a row that has stopped
/// naming its own file, which is exactly the "truncated to useless" shape I6
/// forbids. Twelve leaves `…engine/watch.rs` legible at forty columns, where the
/// counters and the pulse together already want twenty.
const MIN_PATH_WIDTH: usize = 12;

/// The smallest body a second footer line may leave behind.
///
/// Two rows, because that is the shortest thing that still reads as a diff: a
/// file heading and one line under it. Below that the footer would be buying
/// legibility with the content it exists to make legible.
const MIN_BODY: u16 = 2;

/// Shown on the footer while the viewport is moving itself.
///
/// The mockup's own words. It sits with the position rather than with the
/// hints because it is **state**, not advice, and a notice replaces the hints:
/// a reader being told a file could not be read still needs to know whether
/// what they are looking at is live.
const FOLLOWING: &str = "follow ▶";

/// What joins two facts drawn on one line.
///
/// Twice on screen: the header's mode word and its file count, and the empty
/// state's "nothing changed" and the branch it did not change on. The mockup's
/// own character, and the same one the hint bar uses, because two separators
/// would be two dialects for one idea.
///
/// Deliberately **not** [`HINT_SEPARATOR`] itself, which is exported so
/// `tests/legibility.rs` can split the *hint bar* on it. Sharing the constant
/// would let a change to how hints are joined silently reshape the header, and
/// these are two independent choices that happen to agree today.
const FACT_SEPARATOR: &str = " · ";

/// What the body says when there is no diff at all.
///
/// **Not `working tree clean`**, which is what this used to say and which is
/// wrong rather than merely plain. That is git's phrase, and git means
/// index-against-HEAD as well as tree-against-index; this diff is only the
/// second, so a worktree with every change staged draws nothing here and was
/// being told it was clean while `git status` said the opposite. `SPEC.md` §11.1
/// rules the wording. Untracked files are included in the claim, since an
/// untracked file is an unstaged one too.
const NOTHING_CHANGED: &str = "no unstaged changes";

/// The narrowest the text column may get before line numbers are dropped.
///
/// Below this the gutter costs more than it explains, which is the shape of
/// "truncated to useless" that I6 forbids. At forty columns with four-digit line
/// numbers the text still gets thirty-four, so the gutter survives the case the
/// invariant is actually about.
const MIN_TEXT_WIDTH: usize = 24;

/// What the monitor is doing, which is the mockup's `watching` and the set that
/// word implies.
///
/// **Two, and I1 is the reason rather than minimalism.** `SPEC.md` §5.1 read
/// `watching` as implying at least a settling state and an idle one, and it
/// implies neither: both are *durations*, and this shell wakes only when a file
/// changes. [`vigia_core::Watcher::next_tick`] blocks until a burst has settled,
/// so the shell is never awake **during** settling; "idle" would need a wake that
/// says nothing happened, which is the timer I1 forbids. Either word could come
/// into existence and then never leave, which is the frozen clock §11.1 already
/// rejected for the pulse.
///
/// So what is left is the only distinction the shell can actually make: whether
/// what is on screen is still following the tree.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    /// The watch is live, so the screen follows the working tree.
    ///
    /// The default, and that is a ruling rather than an accident of ordering.
    /// The watch is armed *after* first paint, deliberately, so it does not
    /// observe the shell's own setup reads. A third word for those microseconds
    /// would flicker on every launch to describe a state that always resolves the
    /// same way within one wake, and a genuine arming failure arrives as its own
    /// wake and corrects this.
    #[default]
    Watching,
    /// The watch never armed, or it ended, so this is a still picture.
    Lost,
}

impl Mode {
    /// The word the header draws.
    ///
    /// `not watching` rather than `stalled` or `still`: it is the mockup's own
    /// word negated, so a reader who has learned one has learned both. `stalled`
    /// reads as temporary when this is not, and `still` means both "motionless"
    /// and "continuing".
    pub fn word(self) -> &'static str {
        match self {
            Self::Watching => "watching",
            Self::Lost => "not watching",
        }
    }
}

/// What the chrome says that the view itself does not know.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Chrome {
    /// Name of the working tree being watched.
    pub worktree: String,
    /// The branch the empty state names, when there is one.
    ///
    /// `None` covers two different things on purpose, because the empty state
    /// draws them identically: a detached HEAD, which names no branch, and a
    /// frame that has a diff to show and therefore never asked. The second is
    /// what keeps I4 true. Reading `.git/HEAD` on a frame that will not draw the
    /// answer is exactly the shape I4 forbids, so the shell asks only when the
    /// diff is empty. See [`crate::branch_for`].
    pub branch: Option<String>,
    /// Whether the watch is still live.
    ///
    /// Durable, which is why it is here rather than riding [`Chrome::notice`]. A
    /// watch that ends puts the word on the header and its error on the footer:
    /// the header says the diff has stopped being live, the notice says which
    /// failure did it. Before they were split, the durable half rode the notice
    /// alone and survived only because the tick that clears a notice can never
    /// arrive again once the watch is gone.
    pub mode: Mode,
    /// Something the reader should see instead of the key hints.
    ///
    /// A monitor survives a failed frame rather than exiting, so this is where a
    /// missing blob mid-`git gc` or an unreadable file goes. It replaces the
    /// hints because a reader who has just been told something is wrong does not
    /// need reminding that `q` quits.
    pub notice: Option<String>,
    /// Whether the viewport is moving itself to what just changed.
    ///
    /// Drawn, because I5 is otherwise invisible: a view that has not moved
    /// because nothing changed and one that has not moved because following
    /// was switched off look identical, and the reader's next action differs
    /// completely between them.
    pub following: bool,
}

/// `N/M`, or nothing at all when there is no diff to be positioned within.
///
/// One function rather than two because it is built twice per frame from
/// different inputs: [`Footer::plan`] wants the widest position a file count can
/// produce, and the draw wants the real one. Written twice, the two would
/// eventually disagree about a format the layout is measured against.
fn position_of(file: usize, files: usize) -> String {
    if files == 0 {
        String::new()
    } else {
        // Saturating because [`View`] has public fields and [`render`] is public,
        // so nothing stops a caller handing over a position past the end of its
        // own file list. [`View::collect`] always clamps, but the type does not.
        format!("{}/{files}", file.saturating_add(1))
    }
}

/// The state's ladder, widest rung first.
///
/// `follow ▶  N/M`, then the marker alone, then nothing. The position goes
/// before the marker because the header already carries the file count, so
/// `N/M` is the half a reader can reconstruct; whether the view is still live is
/// not recoverable from anywhere else on the screen.
///
/// Always ends in an empty rung, which is what makes [`widest_fitting`] total.
fn state_rungs(following: bool, position: &str) -> Vec<String> {
    let mut rungs = Vec::with_capacity(3);
    match (following, position.is_empty()) {
        // `follow ▶ ` with nothing after it would read as a truncation rather
        // than a state, so a clean worktree gets the marker on its own.
        (true, false) => {
            rungs.push(format!("{FOLLOWING}  {position}"));
            rungs.push(FOLLOWING.to_owned());
        }
        (true, true) => rungs.push(FOLLOWING.to_owned()),
        (false, false) => rungs.push(position.to_owned()),
        (false, true) => {}
    }
    rungs.push(String::new());
    rungs
}

/// `N files`, or nothing at all when there is no diff to count.
///
/// Zero is nothing rather than `0 files`, the same way [`position_of`] is nothing
/// when there is no diff to be positioned within. `0 files` spends columns
/// restating what the empty state below says in words.
fn count_of(files: usize) -> String {
    match files {
        0 => String::new(),
        1 => "1 file".to_owned(),
        n => format!("{n} files"),
    }
}

/// The header's right-hand side, widest rung first.
///
/// `watching · 3 files`, then the mode word alone, then nothing.
///
/// **The count goes first and the mode word is the last rung standing**, which is
/// [`state_rungs`]' rule one line up. The count summarises the body, which is on
/// screen and can be counted by looking; whether the pane is still live is
/// recoverable from nowhere else at all. That ordering matters most at exactly
/// the widths where the body has nothing in it to count, which is the empty state
/// this word exists for.
///
/// **The mode word is therefore never cut**, which is stricter than the marking
/// rule the rest of the header follows: a ladder drops whole rungs, so the word
/// is drawn entire or not drawn. `wat›` is a state a reader cannot read, and
/// unlike a path it has no half that identifies it.
///
/// Always ends in an empty rung, which is what makes [`widest_fitting`] total.
fn header_rungs(mode: Mode, files: usize) -> Vec<String> {
    let word = mode.word();
    let mut rungs = Vec::with_capacity(3);
    let count = count_of(files);
    if !count.is_empty() {
        rungs.push(format!("{word}{FACT_SEPARATOR}{count}"));
    }
    rungs.push(word.to_owned());
    rungs.push(String::new());
    rungs
}

/// The one body line a worktree with no changes gets.
///
/// This is B3, ruled into `SPEC.md` §11.1 with its number left behind in §11.2,
/// and it carries two of that ruling's four facts.
/// The other two are the header's: which repository, from the worktree name, and
/// that it is watching, from the mode word. So the empty state costs one row
/// rather than four, and the mode word is what makes the fourth fact sayable in
/// none at all.
///
/// **The branch is orientation, not the comparison.** Nothing about it changes
/// what is diffed. It is named because two agents on two worktrees of one
/// repository are otherwise identical on screen, which is the multi-worktree case
/// `SPEC.md` §4 defers rather than rejects.
///
/// A detached HEAD drops it rather than inventing one: `HEAD@abc123` would put a
/// commit id in a monitor that shows no commits.
fn empty_state(branch: Option<&str>) -> String {
    match branch {
        Some(branch) => format!("{NOTHING_CHANGED}{FACT_SEPARATOR}{branch}"),
        None => NOTHING_CHANGED.to_owned(),
    }
}

/// One file heading's parts, gathered so [`Painter::file_row`] takes a shape
/// rather than seven positional arguments that a caller could transpose.
struct Heading<'r> {
    kind: char,
    path: &'r str,
    from: Option<&'r str>,
    churn: Option<(u32, u32)>,
    spark: &'r [u16; HISTORY_BUCKETS],
    recency: Recency,
    heat: &'r [HeatBucket; HEAT_BUCKETS],
}

/// Columns something of `width` costs on the right-hand side of a row.
///
/// One more than it measures, because [`Painter::put_right`] leaves a gap so the
/// right-hand text never touches what is drawn from the left. Written once
/// rather than as a `+ 1` at each call site: the two places that reserve space
/// and the one that draws it have to agree, and a `+ 1` remembered in two of
/// three is a row that overwrites its own path at one width in twenty.
fn reserved(width: usize) -> usize {
    if width == 0 { 0 } else { width + 1 }
}

/// What one drawn slice of the heat strip means.
///
/// Public because [`Theme::heat`] resolves it, the same way [`Theme::class`]
/// resolves a syntax class: the shell decides which distinctions are worth a
/// colour here and the theme decides which colour each gets, so
/// [#11](https://github.com/breferrari/vigia/issues/11) can repaint all of this
/// without touching the projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Heat {
    /// Nothing changed in this slice. Drawn as the track rather than skipped,
    /// because a gap would make the strip's own length ambiguous.
    Cool,
    /// Additions only.
    Added {
        /// At least half as busy as the busiest slice of this file.
        heavy: bool,
    },
    /// Removals only.
    Removed {
        /// At least half as busy as the busiest slice of this file.
        heavy: bool,
    },
    /// Both, which `SPEC.md` §5.1 left unruled and §11.1 now rules.
    Mixed {
        /// At least half as busy as the busiest slice of this file.
        heavy: bool,
    },
}

/// Re-project a heat map onto `width` slices and classify each one.
///
/// `width` is a rung of [`HEAT_RUNGS`], so it divides [`HEAT_BUCKETS`] exactly
/// and each drawn slice is the **sum** of the same number of source slices. That
/// is what makes the narrower rung a lower resolution of the whole file rather
/// than a prefix of it.
///
/// `heavy` is measured against the busiest slice **of this file**, and the
/// asymmetry with the sparkline is deliberate. A sparkline is compared *down* a
/// file list, so it shares one scale across the screen; a heat strip is read
/// *across* one row to find where in that file the work is, so its own busiest
/// slice is the only meaningful denominator. `SPEC.md` §11.1 carries both.
///
/// Empty when `width` is zero, and when nothing changed anywhere: a strip of
/// pure track says "this file is in the diff and I cannot tell you where", which
/// is worse than saying nothing and costs twelve columns to say it.
fn heat_at(buckets: &[HeatBucket; HEAT_BUCKETS], width: usize) -> Vec<Heat> {
    if width == 0 || buckets.iter().all(|bucket| bucket.total() == 0) {
        return Vec::new();
    }

    let group = HEAT_BUCKETS / width;
    let summed: Vec<HeatBucket> = buckets
        .chunks(group)
        .map(|chunk| HeatBucket {
            added: chunk.iter().map(|b| b.added).sum(),
            removed: chunk.iter().map(|b| b.removed).sum(),
        })
        .collect();

    let busiest = summed.iter().map(|b| b.total()).max().unwrap_or(0);
    summed
        .iter()
        .map(|bucket| {
            // Half of the busiest, compared without dividing, so an odd busiest
            // does not round a genuinely heavy slice down.
            let heavy = bucket.total() * 2 >= busiest;
            match (bucket.added > 0, bucket.removed > 0) {
                (false, false) => Heat::Cool,
                (true, false) => Heat::Added { heavy },
                (false, true) => Heat::Removed { heavy },
                (true, true) => Heat::Mixed { heavy },
            }
        })
        .collect()
}

/// A path's buckets as glyphs, or `None` when it has no churn to draw.
///
/// `peak` is the busiest bucket **anywhere on the screen**, so two rows drawn
/// side by side can be compared by height. A bucket with anything in it is never
/// blank: it takes the lowest block, because "one write" and "no writes" are the
/// distinction the strip exists to make and rounding the first down to nothing
/// would erase it.
fn spark_of(buckets: &[u16; HISTORY_BUCKETS], peak: u16) -> Option<[char; HISTORY_BUCKETS]> {
    if peak == 0 || buckets.iter().all(|&count| count == 0) {
        return None;
    }
    let mut glyphs = [SPARK_EMPTY; HISTORY_BUCKETS];
    for (glyph, &count) in glyphs.iter_mut().zip(buckets.iter()) {
        if count == 0 {
            continue;
        }
        let scaled = (usize::from(count) * SPARK_RAMP.len()).div_ceil(usize::from(peak));
        *glyph = SPARK_RAMP[scaled.clamp(1, SPARK_RAMP.len()) - 1];
    }
    Some(glyphs)
}

/// The widest rung of `ladder` that fits in `room`.
///
/// Ladders are written widest first, so this is the first that fits. Every
/// ladder ends in an empty rung, so the fallback is unreachable rather than a
/// silent default.
fn widest_fitting<S: AsRef<str>>(ladder: &[S], room: usize) -> &str {
    ladder
        .iter()
        .map(AsRef::as_ref)
        .find(|rung| width_of(rung) <= room)
        .unwrap_or("")
}

/// What the footer will draw, and how many rows it needs.
///
/// Planned rather than drawn, because two callers need the answer before there
/// is anything to draw: [`body_height`] has to know how many rows are left for
/// the body, and [`render`] has to put the body somewhere that does not collide
/// with it. Both go through here with the same inputs, so the row budget and the
/// layout are one computation and cannot drift apart.
struct Footer<'a> {
    /// One, or two when a single line cannot hold both halves. Zero on a screen
    /// with no room for a footer at all.
    rows: u16,
    /// Columns the state may take on its line.
    reserved: usize,
    /// The hints rung, or the notice.
    left: &'a str,
    /// Whether `left` is a notice, which is what decides its colour.
    alert: bool,
}

impl<'a> Footer<'a> {
    /// Decide the footer's shape from the width, the state, and the file count.
    ///
    /// **From the file count, never the scroll position.** `{files}/{files}` is
    /// the widest position that count can produce, so reserving it means the
    /// footer cannot gain or lose a row while a reader scrolls from file 9 to
    /// file 10. A layout that reflowed under scrolling would be worse than one
    /// that is occasionally a column meaner than it had to be, and the meanness
    /// only shows below seventeen columns.
    fn plan(area: Rect, chrome: &'a Chrome, files: usize) -> Self {
        let width = usize::from(area.width);
        if area.height < 2 {
            return Self {
                rows: 0,
                reserved: 0,
                left: "",
                alert: false,
            };
        }

        // The last file's position is the widest this count can produce, since
        // no position numbers higher than the count itself.
        let widest = position_of(files.saturating_sub(1), files);
        let reserved = width_of(widest_fitting(
            &state_rungs(chrome.following, &widest),
            width,
        ));
        // The gap keeps the state from touching the hints, and is only owed when
        // there is a state to keep away from them.
        let taken = if reserved == 0 { 0 } else { reserved + 1 };

        // A second line is worth taking only if it buys something: there has to
        // be a state to move up to it, and a body still worth showing
        // underneath. One header, two footer rows and `MIN_BODY` is the shortest
        // screen where both hold.
        //
        // **Measured against the hints, never against a notice.** A notice is
        // transient — a file that vanished between being named and being read,
        // a repository mid-`git gc` — so letting it decide the height would jog
        // the reader's diff down a row and back every time one flickered. That
        // is the same thing I5 ruled out for a terminal resize: a monitor does
        // not move content for something that expresses no intent. It also means
        // this height is a function of width, follow state and file count alone,
        // so a caller that sampled the chrome before a notice was raised still
        // gets the answer the renderer will use.
        let grows =
            width_of(HINT_RUNGS[0]) + taken > width && reserved > 0 && area.height >= 3 + MIN_BODY;
        let rows = if grows { 2 } else { 1 };

        let room = if grows {
            width
        } else {
            width.saturating_sub(taken)
        };
        // A notice is one token: it takes whatever room the line gives it and
        // marks the cut. The hints are a list, so they drop whole rungs instead.
        let (left, alert) = match &chrome.notice {
            Some(notice) => (notice.as_str(), true),
            None => (widest_fitting(&HINT_RUNGS, room), false),
        };

        Self {
            rows,
            reserved,
            left,
            alert,
        }
    }
}

/// Body height available for rows in this area, which is what a caller has to
/// ask [`View::collect`] for.
///
/// One line goes to the header and one or two to the footer, so this needs the
/// same inputs the footer is planned from: `files` is
/// [`vigia_core::Frame::files`]'s length, which a caller knows before collecting
/// anything and which equals [`View::files`] afterwards.
///
/// Saturating rather than clamped so a one-row terminal asks for nothing instead
/// of underflowing.
pub fn body_height(area: Rect, chrome: &Chrome, files: usize) -> usize {
    let footer = Footer::plan(area, chrome, files);
    usize::from(area.height).saturating_sub(1 + usize::from(footer.rows))
}

/// Draw a whole screen: one header line, the body, and one or two footer lines.
///
/// Any area is legal, including one too short for a body and one column wide. A
/// monitor that panics when a pane is dragged narrow is worse than one that
/// draws something cramped.
pub fn render(buf: &mut Buffer, area: Rect, view: &View, theme: &Theme, chrome: &Chrome) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    // Planned from `view.files`, which on the path that matters is the same
    // number `body_height`'s caller passed: `View::collect` copies it straight
    // off the frame and changes nothing. Where they can differ is a caller
    // redrawing a *stale* view after a failed collect, and that costs nothing,
    // because the plan and the draw below both read this one value and are
    // therefore consistent with each other whatever it holds.
    let footer = Footer::plan(area, chrome, view.files);

    let mut painter = Painter {
        buf,
        theme,
        gutter: 0,
    };

    painter.header(Rect { height: 1, ..area }, view, chrome);

    if footer.rows > 0 {
        painter.footer(area, view, chrome, &footer);
    }

    let rows = area.height.saturating_sub(1 + footer.rows);
    if rows > 0 {
        painter.body(
            Rect {
                y: area.y + 1,
                height: rows,
                ..area
            },
            view,
            chrome,
        );
    }
}

/// A buffer, a palette, and the one measurement the body rows share.
struct Painter<'a> {
    buf: &'a mut Buffer,
    theme: &'a Theme,
    /// Digits reserved for line numbers, or zero when there is no room.
    gutter: usize,
}

impl Painter<'_> {
    /// Write `text` at `x`, clipped to `limit` columns, and return the next
    /// column.
    fn put(&mut self, x: u16, y: u16, text: &str, limit: usize, style: Style) -> u16 {
        if limit == 0 {
            return x;
        }
        let (next, _) = self.buf.set_stringn(x, y, text, limit, style);
        next
    }

    /// Write `text` clipped to `limit`, and say so when it did not fit.
    ///
    /// This is I6's rule for everything that is one token rather than a list:
    /// the worktree name, a notice, a hunk header, a note, the empty-state line
    /// and a line of file content. None of them can drop an item the way the
    /// hint bar can, and none has an identifying half the way a path does, so
    /// the honest thing is to fill the room and mark the edge.
    ///
    /// The mark gets a **reserved** column rather than overwriting the last one,
    /// and the reason is not the one it looks like. Overwriting cannot corrupt
    /// the row: `ratatui` refuses to write into the continuation cell a
    /// two-column glyph covers. What it does instead is **drop the mark**, so a
    /// row filled to its last column by a wide glyph is drawn as one that simply
    /// ends. Reserving the column is what guarantees the mark always lands.
    /// `tests/legibility.rs` gates it, and a plain ASCII line never reaches the
    /// case because it ends on a one-column character.
    fn put_marked(&mut self, x: u16, y: u16, text: &str, limit: usize, style: Style) {
        if limit == 0 || text.is_empty() {
            return;
        }
        if width_of(text) <= limit {
            self.put(x, y, text, limit, style);
            return;
        }
        // `limit` is always derived from a screen width, so it fits in a `u16`.
        self.put(x, y, text, limit - 1, style);
        self.buf
            .set_stringn(x + limit as u16 - 1, y, CONTINUES, 1, style);
    }

    /// Write a sequence of styled runs under **one** limit, marking the edge.
    ///
    /// The many-run form of [`Painter::put_marked`], and it has to be one call
    /// rather than a `put_marked` per run. The limit belongs to the row, but the
    /// mark belongs to whichever run happens to reach the edge, and a per-run
    /// version would either mark every run or none of them.
    ///
    /// The mark gets a reserved column for exactly the reason `put_marked` gives:
    /// `ratatui` refuses to write into the continuation cell of a two-column
    /// glyph, so overwriting the last column silently **drops** the mark and a
    /// clipped line is drawn as one that simply ends. `tests/legibility.rs`
    /// sweeps every width for that.
    ///
    /// `total` is the runs' width, which the caller already accumulated while
    /// building them. Passed in rather than re-derived: measuring it here walks
    /// every line a second time and undoes the ASCII fast path [`printable`]
    /// exists for.
    fn put_runs_marked(
        &mut self,
        x: u16,
        y: u16,
        runs: &[(String, Style)],
        total: usize,
        limit: usize,
    ) {
        if limit == 0 {
            return;
        }

        let overflows = total > limit;
        let budget = if overflows { limit - 1 } else { limit };

        // The style the mark inherits: whichever run ran out of room, so a
        // clipped comment is marked in the comment's colour rather than in
        // whatever the row started with.
        //
        // Seeded from the **first** run rather than from a theme default,
        // because at `limit == 1` the budget is zero and the loop below writes
        // nothing at all. Seeded with `context` instead, a one-column diff row
        // marked in `Reset` where `put_marked` on the same content would have
        // used the caller's style, and the two spellings of one rule had already
        // drifted apart at the one width nothing tests.
        let mut marked_in = runs.first().map_or(self.theme.context, |(_, style)| *style);
        let end = x + budget as u16;
        let mut at = x;
        for (text, style) in runs {
            if at >= end {
                break;
            }
            marked_in = *style;
            at = self.put(at, y, text, usize::from(end - at), *style);
        }

        if overflows {
            self.buf
                .set_stringn(x + limit as u16 - 1, y, CONTINUES, 1, marked_in);
        }
    }

    /// Write `text` so that it ends at the right edge of `area`.
    ///
    /// Dropped entirely rather than truncated when it does not fit. Half a count
    /// on the right of a line is noise; its absence is at least honest.
    fn put_right(&mut self, area: Rect, text: &str, style: Style) -> usize {
        let width = width_of(text);
        if width == 0 || width > usize::from(area.width) {
            return 0;
        }
        self.buf.set_stringn(
            area.x + area.width - width as u16,
            area.y,
            text,
            width,
            style,
        );
        // The gap keeps the right-hand text from touching whatever is drawn from
        // the left, which at forty columns happens constantly.
        width + 1
    }

    /// One line of chrome: something on the left, something on the right, and the
    /// right-hand side wins the space.
    ///
    /// The header and the footer are the same shape, and having them share it is
    /// not only brevity. The right-hand text is placed first so that a long name
    /// on the left loses characters to it rather than the other way round: the
    /// number is what changes, and what changes is what a glance is for. Written
    /// twice, one of them eventually stops doing that.
    fn status_line(
        &mut self,
        area: Rect,
        left: &str,
        style: Style,
        right: &str,
        right_style: Style,
    ) {
        self.buf.set_style(area, self.theme.chrome_dim);
        let taken = self.put_right(area, right, right_style);
        let room = usize::from(area.width).saturating_sub(taken);
        self.put_marked(area.x, area.y, left, room, style);
    }

    fn header(&mut self, area: Rect, view: &View, chrome: &Chrome) {
        // The worktree name and nothing else on the left, which is the one place
        // the layout departs from `assets/preview.svg` on purpose: a title bar
        // reading `vigia` spends six of forty columns telling the reader which
        // program they started, and what they cannot tell by looking is which
        // *tree*. `SPEC.md` §11.1 carries the argument, because §5.1's rule is
        // that a published artifact answering a question is the answer, so a
        // deliberate departure from one has to be written down or it reads as
        // drift.
        //
        // The header never takes a second line the way the footer does. A name
        // is not a list and has nowhere to break, so a second line could not
        // guarantee a fit and would spend a body row on a maybe. The right-hand
        // side breaks instead, by dropping whole rungs.
        let rungs = header_rungs(chrome.mode, view.files);
        let right = widest_fitting(&rungs, usize::from(area.width));
        // **A dead watch has to be visible, not merely present.** Drawn in the
        // same dim grey as the count, `not watching` is a word a reader has to
        // go looking for, and a monitor whose failure state looks exactly like
        // its working one has failed twice. `SPEC.md` §5 makes colour half the
        // differentiator, so the abnormal state is loud and the normal one stays
        // quiet.
        //
        // The **footer's own** alert rather than a colour of its own: the notice
        // carrying which failure already uses it, and the two halves of one
        // event should not arrive in two different reds. A reuse of an existing
        // style rather than a palette decision, which stays #11's.
        let right_style = match chrome.mode {
            Mode::Watching => self.theme.chrome_dim,
            Mode::Lost => self.theme.alert,
        };
        self.status_line(
            area,
            &chrome.worktree,
            self.theme.chrome,
            right,
            right_style,
        );
    }

    /// The footer, on the bottom one or two rows of `area`.
    ///
    /// Takes the whole area rather than its own rows, because which rows it owns
    /// is what [`Footer::plan`] decided.
    fn footer(&mut self, area: Rect, view: &View, chrome: &Chrome, footer: &Footer<'_>) {
        let position = position_of(view.top.file, view.files);
        // Clamped to what was reserved, not to the width. The plan handed the
        // rest of the line to the hints, and the drawn position can be narrower
        // than the widest one reserved for it, so a state sized to the width
        // would draw over them.
        let rungs = state_rungs(chrome.following, &position);
        let state = widest_fitting(&rungs, footer.reserved);

        let style = if footer.alert {
            self.theme.alert
        } else {
            self.theme.chrome_dim
        };
        let bottom = Rect {
            y: area.y + area.height - 1,
            height: 1,
            ..area
        };

        if footer.rows == 2 {
            // State above, hints below. The hints keep the bottom row they had
            // at eighty columns, so narrowing a pane moves the new line in
            // rather than moving the old one out from under the reader.
            let upper = Rect {
                y: bottom.y - 1,
                ..bottom
            };
            self.status_line(
                upper,
                "",
                self.theme.chrome_dim,
                state,
                self.theme.chrome_dim,
            );
            self.status_line(bottom, footer.left, style, "", self.theme.chrome_dim);
        } else {
            self.status_line(bottom, footer.left, style, state, self.theme.chrome_dim);
        }
    }

    fn body(&mut self, area: Rect, view: &View, chrome: &Chrome) {
        if view.files == 0 {
            self.put_marked(
                area.x,
                area.y,
                &empty_state(chrome.branch.as_deref()),
                usize::from(area.width),
                self.theme.chrome_dim,
            );
            return;
        }

        self.gutter = gutter_width(view, usize::from(area.width));
        for (offset, row) in view.rows.iter().take(usize::from(area.height)).enumerate() {
            let y = area.y + offset as u16;
            match row {
                Row::File {
                    path,
                    from,
                    kind,
                    churn,
                    spark,
                    recency,
                    heat,
                } => self.file_row(
                    Rect { y, ..area },
                    &Heading {
                        kind: *kind,
                        path,
                        from: from.as_deref(),
                        churn: *churn,
                        spark,
                        recency: *recency,
                        heat,
                    },
                    view.peak,
                ),
                Row::Hunk {
                    old_start,
                    old_lines,
                    new_start,
                    new_lines,
                } => {
                    let text = format!(
                        "@@ -{} +{} @@",
                        span(*old_start, *old_lines),
                        span(*new_start, *new_lines)
                    );
                    // Marked rather than clipped, and this is the row where it
                    // matters most: `@@ -258,7 +25` is not a shortened header,
                    // it is a header naming a different line.
                    self.put_marked(area.x, y, &text, usize::from(area.width), self.theme.hunk);
                }
                Row::Note(note) => {
                    let text = format!("  {note}");
                    self.put_marked(area.x, y, &text, usize::from(area.width), self.theme.note);
                }
                Row::Line {
                    kind,
                    number,
                    text,
                    spans,
                } => {
                    self.line_row(Rect { y, ..area }, *kind, *number, text, spans);
                }
            }
        }
    }

    /// `M src/frame.rs        ● just changed  ▁▃█▅▂▁▁▁      +12 -3`
    ///
    /// Everything to the right of the path is placed right to left, and the
    /// order it is *allocated* in is not the order it is drawn in. Allocation is
    /// priority: the counters first because they are the row's content, then the
    /// pulse, then the sparkline. The pulse outranks the strip for the same
    /// reason `f follow` is the last hint standing, and because its narrow rung
    /// costs one column against the strip's four.
    ///
    /// Nothing is allowed to take the path below [`MIN_PATH_WIDTH`]. A glance
    /// element that cost a reader the name of the file would be spending the
    /// content to decorate it.
    fn file_row(&mut self, area: Rect, heading: &Heading<'_>, peak: u16) {
        let mut right = area;

        let counts = heading
            .churn
            .map(|(added, removed)| format!("+{added} -{removed}"))
            .unwrap_or_default();
        let taken = self.put_right(right, &counts, self.theme.chrome_dim);
        right.width = right.width.saturating_sub(taken as u16);

        // What is left after the kind letter and the path's floor. Saturating,
        // so a row too narrow to hold both simply has no glance budget at all.
        let mut budget = usize::from(right.width).saturating_sub(2 + MIN_PATH_WIDTH);

        let pulse = if heading.recency == Recency::Pulse {
            widest_fitting(&PULSE_RUNGS, budget)
        } else {
            ""
        };
        budget = budget.saturating_sub(reserved(width_of(pulse)));

        // The heat strip outranks the sparkline for what is left. Both are
        // glance elements and only one of them is about the diff on screen: the
        // strip says where in *this* file the change the reader is looking at
        // sits, and the sparkline says how busy the file was before any of it
        // was drawn.
        let heat = HEAT_RUNGS
            .iter()
            .copied()
            .find(|&rung| reserved(rung) <= budget)
            .map(|rung| heat_at(heading.heat, rung))
            .unwrap_or_default();
        budget = budget.saturating_sub(reserved(heat.len()));

        // Drawn right to left, so each block knows where the one outside it
        // ended. The strip drawn is the **tail** of the window: dropping buckets
        // means dropping the oldest, and the oldest are on the left.
        if let Some(strip) = spark_of(heading.spark, peak) {
            let buckets = SPARK_RUNGS
                .iter()
                .copied()
                .find(|&rung| reserved(rung) <= budget)
                .unwrap_or(0);
            if buckets > 0 {
                let tail: String = strip[HISTORY_BUCKETS - buckets..].iter().collect();
                let taken = self.put_right(right, &tail, self.theme.spark);
                right.width = right.width.saturating_sub(taken as u16);
            }
        }
        if !heat.is_empty() {
            // Cell by cell rather than as one string: every slice is the same
            // glyph and only the style differs, which is the whole design.
            let x = right.x + right.width - heat.len() as u16;
            for (offset, slice) in heat.iter().enumerate() {
                self.buf.set_stringn(
                    x + offset as u16,
                    right.y,
                    HEAT_BLOCK.to_string(),
                    1,
                    self.theme.heat(*slice),
                );
            }
            right.width = right.width.saturating_sub(reserved(heat.len()) as u16);
        }
        if !pulse.is_empty() {
            let taken = self.put_right(right, pulse, self.theme.pulse);
            right.width = right.width.saturating_sub(taken as u16);
        }

        let mut room = usize::from(right.width);
        let letter = format!("{} ", heading.kind);
        let x = self.put(area.x, area.y, &letter, room, self.theme.kind);
        room = room.saturating_sub(usize::from(x - area.x));

        let mut label = heading.path.to_owned();
        if let Some(from) = heading.from {
            // Which file it *was* is the whole content of a rename, so it is
            // part of the label rather than something to reveal on a keypress.
            label.push_str(" ← ");
            label.push_str(from);
        }
        self.put(
            x,
            area.y,
            &elide_head(&label, room),
            room,
            self.theme.recency(heading.recency),
        );
    }

    /// `  128 +    let value = 1;`
    ///
    /// **The sigil carries the diff, the text carries the syntax**, which is
    /// `SPEC.md` §11.1's ruling and the mockup's own layout: added, removed and
    /// context lines are highlighted identically, and only the `+` or `-` says
    /// which is which. What the picture uses to make that legible is a row
    /// background tint, and sixteen foreground-only colours cannot draw one, so
    /// the signal here is thinner than in the picture until #11 lands a
    /// truecolour path.
    fn line_row(&mut self, area: Rect, kind: LineKind, number: u32, text: &str, spans: &[Span]) {
        let (diff, sigil) = match kind {
            LineKind::Added => (self.theme.added, '+'),
            LineKind::Removed => (self.theme.removed, '-'),
            LineKind::Context => (self.theme.context, ' '),
        };

        let mut x = area.x;
        let mut room = usize::from(area.width);
        if self.gutter > 0 {
            let gutter = self.gutter;
            let numbered = format!("{number:>gutter$} ");
            x = self.put(x, area.y, &numbered, room, self.theme.gutter);
            room = room.saturating_sub(gutter + 1);
        }

        let mut runs = Vec::with_capacity(spans.len() + 2);
        runs.push((sigil.to_string(), diff));

        // Tab stops are counted from the start of the line's own content, not
        // from the left edge of the screen. The gutter and the sigil shift every
        // row by the same amount, so including them would align tabs to the
        // buffer and leave the file's indentation looking nothing like it does in
        // an editor. The counter therefore runs **across** span boundaries: a tab
        // in the middle of a line advances to the next stop measured from the
        // line's own start, not from the start of whatever run it landed in.
        let mut column = 0usize;
        let mut at = 0usize;
        for span in spans {
            let end = (at + span.len).min(text.len());
            let Some(piece) = text.get(at..end) else {
                // A span boundary that is not a character boundary. It should not
                // happen and it must not panic, because the alternative to one
                // uncoloured line is a monitor that dies on a file. The rest of
                // the line is drawn unclassified.
                break;
            };
            if !piece.is_empty() {
                runs.push((printable(piece, &mut column), self.theme.class(span.class)));
            }
            at = end;
        }
        if at < text.len() {
            // Whatever the spans did not reach, which is the whole line when
            // there are none: an unrecognised file type, or a row a test built
            // by hand.
            //
            // Styled through `class` rather than reaching for `context`
            // directly, so that "an empty span list and one `Plain` span reach
            // the screen identically" stays one rule instead of two expressions
            // that happen to agree. #11 gives the classes their own palette, and
            // the first `Plain` that is not `context` would otherwise leave this
            // path quietly on the old colour.
            runs.push((
                printable(&text[at..], &mut column),
                self.theme.class(Class::Plain),
            ));
        }

        // Content is the one thing that can neither break nor elide: wrapping it
        // would move every line below it, and no part of a line is its
        // identifying part the way a path's tail is. So it says it continues and
        // nothing more. `SPEC.md` §11.1 rules that this is not what I6 means by
        // a truncated label.
        // The sigil is one column and is pushed before the counter starts, so
        // the runs' total width is the counter plus it.
        self.put_runs_marked(x, area.y, &runs, column + 1, room);
    }
}

fn width_of(text: &str) -> usize {
    TextSpan::raw(text).width()
}

/// One side of a hunk header, in git's own shorthand.
///
/// Git omits the count when a side covers exactly one line, and a reader
/// calibrated on `git diff` reads its absence as "one". Reproducing that is
/// cheaper than teaching them a second dialect.
fn span(start: u32, lines: u32) -> String {
    if lines == 1 {
        format!("{start}")
    } else {
        format!("{start},{lines}")
    }
}

/// Digits to reserve for line numbers, or zero to draw none.
///
/// Sized from the largest number actually on screen rather than from the file,
/// so the gutter does not widen for content nobody can see.
fn gutter_width(view: &View, width: usize) -> usize {
    let largest = view
        .rows
        .iter()
        .filter_map(|row| match row {
            Row::Line { number, .. } => Some(*number),
            _ => None,
        })
        .max()
        .unwrap_or(0);

    let digits = largest.max(1).ilog10() as usize + 1;
    // The gutter costs its digits plus a space, and the sigil costs one more.
    if width.saturating_sub(digits + 2) >= MIN_TEXT_WIDTH {
        digits
    } else {
        0
    }
}

/// Keep the tail of `text`, marking the loss, when it will not fit.
///
/// The tail, because the end of a path is the part that identifies the file. A
/// column reading `crates/vigia-core/…` names nothing, which is exactly the
/// truncated-to-useless label I6 forbids. This is the **only** direction on the
/// screen that keeps its end rather than its start, and it is why the two marks
/// are different characters: a path says its head is gone, everything else says
/// its tail continues.
///
/// One column is enough to say so. At `room == 1` the whole path is gone and the
/// result is a bare [`ELIDED`], which is honest about naming nothing rather than
/// showing an arbitrary first character as if it were a name.
fn elide_head(text: &str, room: usize) -> String {
    if width_of(text) <= room {
        return text.to_owned();
    }
    if room == 0 {
        return String::new();
    }

    // Walked backwards, accumulating width, rather than forwards testing each
    // suffix. The forward form re-measures the whole tail once per candidate,
    // which is quadratic in the path length and runs once per visible file row.
    let budget = room - 1;
    let mut start = text.len();
    let mut kept_width = 0usize;
    for (i, c) in text.char_indices().rev() {
        let next = kept_width + width_of(&text[i..i + c.len_utf8()]);
        if next > budget {
            break;
        }
        kept_width = next;
        start = i;
    }

    let mut kept = String::with_capacity(text.len() - start + ELIDED.len_utf8());
    kept.push(ELIDED);
    kept.push_str(&text[start..]);
    kept
}

/// Make one line of file content safe to write into terminal cells.
///
/// Two hazards, both from content nobody wrote for a display. A tab occupies one
/// cell and advances nothing, so everything after it in the row sits at the
/// wrong column. And a control character written straight through can move the
/// cursor or open an escape sequence, which corrupts the whole screen rather
/// than one row.
///
/// Columns are counted from the start of the **line**, which is where the file
/// counts them from too, so `column` is threaded in by the caller and carried
/// across the runs one line is made of. A per-run counter would reset at every
/// syntax boundary and align a tab to the token before it rather than to the
/// line, which is invisible until a file indents with tabs and then wrong on
/// every row of it.
fn printable(text: &str, column: &mut usize) -> String {
    let mut out = String::with_capacity(text.len());
    for (i, c) in text.char_indices() {
        match c {
            '\t' => {
                let stop = TAB_STOP - (*column % TAB_STOP);
                out.extend(std::iter::repeat_n(' ', stop));
                *column += stop;
            }
            c if c.is_control() => {
                out.push(UNPRINTABLE);
                *column += 1;
            }
            c if c.is_ascii() => {
                out.push(c);
                *column += 1;
            }
            c => {
                out.push(c);
                // Only the non-ASCII tail pays for a width lookup, which keeps
                // the common line off the measuring path entirely.
                *column += width_of(&text[i..i + c.len_utf8()]);
            }
        }
    }
    out
}
