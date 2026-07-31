//! I9, held against the shell rather than against the core.
//!
//! > Steady-state frame time holds 60fps under continuous edits. **< 16ms** p99.
//!
//! `crates/vigia-core/tests/budgets.rs` already gates this over the frame path,
//! and that gate is now structurally blind to part of the cost: the core does no
//! highlighting, so a frame measured there is a frame with the syntax parser
//! missing. Highlighting happens where the screen is, because it follows the
//! viewport, so the number that matters is measured here.
//!
//! It is the same shape as `reads.rs`: an invariant the engine can only make
//! *possible* gets a second gate over the caller (`SPEC.md` §7). The difference
//! is the tier. `reads.rs` is structural and takes no slack; this is an absolute
//! wall-clock gate, so it runs release-only and accepts `VIGIA_BUDGET_SLACK`.
//!
//! What it costs, measured while it was written: one screenful of Rust is about
//! 1.5ms of `syntect`, against a frame path that was 6.97ms p99 on the same
//! fixture before highlighting existed.
//!
//! **Every gate here paints, and until [#45](https://github.com/breferrari/vigia/issues/45)
//! none of them did.** A frame timed as `Frame::advance` plus `App::view` is the
//! frame the shell has minus the half that writes cells, and that half is where a
//! row's width is decided. A row carrying seven times more line than pane
//! therefore passed a 16ms gate for two phases, because no gate on either tier
//! could see it. `SPEC.md` §7 now says so as a rule; `tests/paint.rs` holds the
//! structural half.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use vigia::{Action, App, PaintStats, Row, Theme, WHEEL_ROWS, body_height, render};
use vigia_core::{CHECKPOINT_STRIDE, Frame, Highlighter, History, LineKind, Samples};

use support::{
    Scratch, WIDE_EXT, WIDE_UNPARSED_EXT, budget, delta, exclusively_timed, highlight_delta, settle,
};

/// I9: steady-state frame time.
const I9_FRAME: Duration = Duration::from_millis(16);

/// The fixture the core's own I9 gate uses, so the two numbers are comparable.
///
/// 100 x 500 rewritten line for line is the 100k-line diff the budgets are
/// written against. Every file is one hunk of just over a thousand lines, which
/// is also the worst case for highlighting: a screenful is drawn from the top of
/// a hunk whose content changes on every frame.
const FILES: usize = 100;
const LINES: usize = 500;

/// The file rewritten before every frame, which is the one the view is sitting
/// on.
const EDITED_PATH: &str = "src/mod_0.rs";

/// Timed frames between bulk rewrites in the gate at the bottom of this file.
///
/// The settle margin is a fixed two seconds of wall clock and the frame rate is
/// whatever the machine gives, so "sample 250 frames and they will all be inside
/// the margin" is a race rather than an invariant. Measured: one rewrite for the
/// whole sample ran **1.79s against the 2s margin**, which is 1.12x and is not
/// headroom, so a slower runner would settle part-way through and the tail would
/// stop being the event. Rewriting before *every* frame is the opposite mistake
/// and is recorded at the loop itself.
///
/// That 1.79s was taken before [`exclusively_timed`] existed, so it carries the
/// contention of two neighbouring gates; serialised, the same single-rewrite
/// window is nearer 670ms and about 3x. Quoted as measured rather than adjusted,
/// and the conclusion is unchanged either way: 3x is still a number that depends
/// on the runner, and a CI machine is allowed to be three times slower than this
/// one by `VIGIA_BUDGET_SLACK` alone.
///
/// Fifty puts a chunk at **308ms** measured, so the premise holds with better
/// than six times to spare and the gate stops depending on how fast the runner
/// is, which is the whole reason to prefer a frame count over a duration here.
const REWRITE_EVERY: usize = 50;

/// Frames discarded before sampling, and frames sampled.
///
/// The same numbers the core's gate uses, and for the same two reasons: I9 is a
/// claim about steady state so the cold path is out of scope by definition, and
/// a percentile needs enough samples to be one.
const WARMUP_FRAMES: usize = 50;
const SAMPLED_FRAMES: usize = 250;

/// An ordinary terminal.
fn area() -> Rect {
    Rect::new(0, 0, 80, 24)
}

fn body(app: &App, files: usize) -> usize {
    body_height(area(), &app.chrome("fixture", None), files)
}

/// One frame of the shell, timed whole: diff, collect, paint.
///
/// The paint is the half that was missing. `Shell::draw` does exactly this plus
/// a `branch_for` (which reads nothing on a populated frame, by I4) and a
/// terminal size query, so what is timed here is the shipped frame with the tty
/// removed — which is the same carve-out `soak.rs` already names.
fn shell_frame(
    frame: &mut Frame,
    app: &mut App,
    highlighter: &mut Highlighter,
    history: &History,
    buf: &mut Buffer,
    theme: &Theme,
    height: usize,
) {
    frame.advance().expect("advance");
    let chrome = app.chrome("fixture", None);
    let view = app.view(frame, highlighter, history, height).expect("view");
    render(buf, area(), &view, theme, &chrome);
}

/// Whether the absolute wall-clock gate should assert.
///
/// A debug build is several times slower than the one the budgets were set
/// against. Reported rather than silently skipped.
fn absolute_gates_apply() -> bool {
    if cfg!(debug_assertions) {
        eprintln!(
            "note: the absolute budget gate is skipped in a debug build; \
             run `cargo test --release --test budgets` to enforce it"
        );
        false
    } else {
        true
    }
}

/// How long `work` took, for a stage whose result nothing downstream needs.
fn time(work: impl FnOnce()) -> Duration {
    timed(work).1
}

/// [`time`], for a stage that produces something the next stage needs.
fn timed<T>(work: impl FnOnce() -> T) -> (T, Duration) {
    let start = Instant::now();
    let value = work();
    (value, start.elapsed())
}

#[test]
fn a_real_frame_with_highlighting_holds_the_frame_budget() {
    frame_budget_at_depth("shell-i9", 0);
}

#[test]
fn a_frame_holds_the_budget_however_deep_the_reader_has_scrolled() {
    // The case the gate above is structurally blind to, and it is not exotic:
    // `App::new()` starts at row zero, so measuring only there measures the
    // cheapest position of the shape being tested.
    //
    // Highlighting a hunk is forward-only, so drawing row N needs the N rows
    // above it parsed. That is paid once while the hunk is stable. Under
    // continuous edits it is not stable: the file being written is the file
    // being read, its hunk changes before every frame, and a reader who scrolled
    // in to follow along pays the whole walk on every tick with no input at all.
    // Measured before the rewind existed: 29ms p50 and 53ms p99 here, against a
    // 16ms budget, sustained.
    //
    // The depth is inside the *first* hunk rather than across files. Each file
    // is five hundred rewritten lines, so one hunk is a thousand display rows
    // and there is no file boundary to reset anything.
    frame_budget_at_depth("shell-i9-deep", 500);
}

/// Sample the frame budget with the viewport `depth` rows into the diff.
///
/// One function rather than two tests with a constant swapped, because the two
/// depths have to agree about every other term for the comparison to mean
/// anything.
fn frame_budget_at_depth(name: &str, depth: usize) {
    let scratch = Scratch::large_diff(name, FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);
    assert_eq!(frame.files().len(), FILES, "fixture is not {FILES} files");

    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let mut history = History::new();
    let height = body(&app, FILES);

    if depth > 0 {
        // A manual scroll, which disengages follow exactly as a reader's would
        // (`SPEC.md` §11.1). The view then stays where it was put while the
        // edits keep landing, which is the whole point.
        app.apply(
            vigia::Action::Scroll(isize::try_from(depth).expect("a sane depth")),
            &mut frame,
            height,
        )
        .expect("scroll");
        let view = app
            .view(&mut frame, &mut highlighter, &history, height)
            .expect("view");
        assert_eq!(
            view.top.row, depth,
            "the scroll landed at row {} rather than {depth}, so the fixture \
             does not have one hunk deep enough to measure",
            view.top.row
        );
        assert_eq!(view.top.file, 0, "the scroll crossed into another file");
    }

    if !absolute_gates_apply() {
        return;
    }

    let _timed = exclusively_timed();

    // "Under continuous edits", taken literally and the same way the core's gate
    // takes it: one line is rewritten before every frame, so each frame
    // revalidates ninety-nine files, recomputes the one that moved, and
    // re-highlights the one hunk on screen. The edit stands in for the agent in
    // the other pane and is deliberately outside the timed region.
    let mut edits = 0usize;
    let mut marker = String::new();
    let theme = Theme::default();
    let mut buf = Buffer::empty(area());
    let mut next_frame =
        |frame: &mut Frame, app: &mut App, highlighter: &mut Highlighter, history: &mut History| {
            marker = format!("fn edited_{edits}() {{ let value = {edits}; }}");
            scratch.edit_line(EDITED_PATH, 0, &marker);
            edits += 1;
            time(|| {
                // Inside the timed region on purpose. `vigia::run` samples the
                // history on the same wake that advances the frame, so a gate that
                // recorded outside `time` would be timing a frame path the product
                // does not have. It is what I10 costs per tick, measured where I9
                // can see it.
                history.record([EDITED_PATH], Instant::now());
                shell_frame(frame, app, highlighter, history, &mut buf, &theme, height);
            })
        };

    for _ in 0..WARMUP_FRAMES {
        next_frame(&mut frame, &mut app, &mut highlighter, &mut history);
    }

    let before = highlighter.stats();
    let mut frames = Samples::new(SAMPLED_FRAMES);
    for _ in 0..SAMPLED_FRAMES {
        frames.push(next_frame(
            &mut frame,
            &mut app,
            &mut highlighter,
            &mut history,
        ));
    }
    let cost = highlight_delta(before, highlighter.stats());

    // Non-vacuity, in the three directions this gate can be hollow.
    //
    // Highlighting has to have actually happened, or this measures the frame
    // path the core already gates and calls it a shell number.
    assert!(
        cost.lines > 0,
        "no lines were highlighted across {SAMPLED_FRAMES} frames, so this gate \
         is measuring the core's frame path and nothing else"
    );

    // The screen has to have been full, or a frame that drew two rows would be
    // a cheap frame for a reason that is not the code.
    let view = app
        .view(&mut frame, &mut highlighter, &history, height)
        .expect("view");
    assert_eq!(
        view.rows.len(),
        height,
        "the body drew {} of {height} rows, so the frames above were not full \
         screens",
        view.rows.len()
    );

    // And the edits have to be still landing. Checked against the frame's diff
    // rather than against the screen, and the difference is the fixture: this
    // one rewrites every line, so a file's hunk is five hundred removals
    // followed by five hundred additions and the newest line sits far below any
    // viewport. A screen assertion here would fail while the code was perfect.
    let last = marker.clone();
    let shared = frame
        .files()
        .iter()
        .position(|change| change.path == EDITED_PATH)
        .expect("the edited file is still a change");
    let (_, diff) = frame.diff(shared).expect("diff");
    assert!(
        diff.hunks.iter().any(|hunk| hunk
            .lines
            .iter()
            .any(|line| line.kind == LineKind::Added && line.text == last)),
        "the diff for {EDITED_PATH} does not contain {last:?}, so the edits \
         stopped reaching it"
    );

    // The highlighter has to be re-parsing every frame, which is what says the
    // edits reach *it* and not merely the diff. One hunk is on screen and its
    // content changes before every frame, so the steady state is exactly one
    // re-parse per frame and no reuse at all.
    assert_eq!(
        cost.parsed, SAMPLED_FRAMES as u64,
        "{} hunks were re-parsed across {SAMPLED_FRAMES} frames, so the visible \
         hunk is not changing under the highlighter and this is not the steady \
         state I9 describes",
        cost.parsed
    );

    // And the cost has to follow the screen rather than the hunk, at any depth.
    //
    // The bound is a screenful **plus one checkpoint stride**, not a screenful:
    // a hunk whose content changed rewinds to the deepest parse position the new
    // content still agrees with, and that position sits at worst a whole stride
    // above the first drawn row. Without the rewind this number was the reader's
    // scroll depth, which is the 53ms-per-frame shape it exists to avoid.
    let per_frame = cost.lines / SAMPLED_FRAMES as u64;
    let bound = (height + CHECKPOINT_STRIDE) as u64;
    assert!(
        per_frame <= bound,
        "{per_frame} lines were highlighted per frame for a {height}-row body at \
         depth {depth}, over the {bound} a rewind to the last checkpoint can \
         cost, so a frame is parsing more of the hunk than it draws"
    );

    let p99 = frames.percentile(0.99).expect("samples");
    assert!(
        p99 <= budget(I9_FRAME),
        "I9: a real frame with highlighting over {FILES} files was {p99:?} p99, \
         over the {:?} budget (p50 {:?}, max {:?}, {} hunks parsed, {} reused, \
         {} lines, {} bytes)",
        budget(I9_FRAME),
        frames.percentile(0.50).expect("samples"),
        frames.max().expect("samples"),
        cost.parsed,
        cost.reused,
        cost.lines,
        cost.bytes
    );
}

#[test]
fn the_frame_budget_holds_through_a_bulk_rewrite() {
    // The third position in this gate's input space, after "at the top" and "deep
    // in a hunk". Those two vary *where* the window is; this one varies *when*,
    // and it is the axis `SPEC.md` §7 gained from this test existing: a gate that
    // settles before it measures has measured the cheapest state.
    //
    // The event is a formatter, a branch switch or a multi-file agent edit. Every
    // file changes at once, so for the whole settle margin no file can be proved
    // unchanged and every one the shell asks for is recomputed rather than
    // reused. §10 claimed that breaks I9 for about two seconds. It does over the
    // core frame path, whose fixture materialises all hundred files: 98 of 182
    // frames over budget, 22.34ms p99. The shell recomputes only what it draws,
    // which is why this gate can exist at all, and `reads.rs` holds that half
    // structurally. This is the wall clock agreeing.
    let scratch = Scratch::large_diff("shell-i9-bulk", FILES, LINES);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);
    assert_eq!(frame.files().len(), FILES, "fixture is not {FILES} files");

    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let mut history = History::new();
    let height = body(&app, FILES);

    if !absolute_gates_apply() {
        return;
    }

    let _timed = exclusively_timed();

    let theme = Theme::default();
    let mut buf = Buffer::empty(area());
    let mut draw =
        |frame: &mut Frame, app: &mut App, highlighter: &mut Highlighter, history: &mut History| {
            time(|| {
                history.record([EDITED_PATH], Instant::now());
                shell_frame(frame, app, highlighter, history, &mut buf, &theme, height);
            })
        };

    // Warm up *before* the first rewrite, not after. Warming afterwards would
    // spend the margin this gate exists to measure and leave the samples in the
    // settled state every other gate here already covers.
    for _ in 0..WARMUP_FRAMES {
        draw(&mut frame, &mut app, &mut highlighter, &mut history);
    }

    // The event, re-established every `REWRITE_EVERY` frames, and the frame that
    // absorbs its write-back deliberately not timed.
    //
    // Both halves were measured rather than chosen. One rewrite for the whole
    // sample is not enough: the window ran 1.79s against a 2s margin, 1.12x
    // headroom, so a runner any slower would settle part-way and the tail would
    // stop being the event. Rewriting before every frame is worse: `rewrite_all`
    // writes about 1.5 MiB, and while the call sits outside `time` its write-back
    // does not, so thirteen of them took a 2.67ms p50 to a 27ms p99 and this gate
    // measured the fixture. It also starved the two gates above, which share this
    // binary and are timed: all three failed together and the other two passed
    // under `--test-threads=1`.
    //
    // So the rewrite is periodic, and the one frame that pays for the harness's
    // own write-back is spent untimed. `SPEC.md` §7 already puts the cold path
    // outside I9 by definition, and this is that: the cost of a test fixture
    // hitting the disk is not a cost of the shell.
    let before = frame.stats();
    let highlighted = highlighter.stats();
    let mut frames = Samples::new(SAMPLED_FRAMES);
    for at in 0..SAMPLED_FRAMES {
        if at % REWRITE_EVERY == 0 {
            scratch.rewrite_all(FILES, LINES, at / REWRITE_EVERY + 1);
            draw(&mut frame, &mut app, &mut highlighter, &mut history);
        }
        // The drawn file, rewritten before each frame. One file rather than a
        // hundred, the idiom the two gates above already use, and the term that
        // decides frame cost, since only drawn files are fingerprinted at all.
        scratch.edit_line(
            EDITED_PATH,
            0,
            &format!("fn bulk_edited_{at}() {{ let value = {at}; }}"),
        );
        frames.push(draw(&mut frame, &mut app, &mut highlighter, &mut history));
    }
    let cost = delta(before, frame.stats());
    let parsed = highlight_delta(highlighted, highlighter.stats());

    // Non-vacuity, first in the direction this whole file exists for. Highlighting
    // has to have actually happened, or this measures the frame path the core
    // already gates and reports it as a shell number. One re-parse per frame is
    // the floor: the drawn file's hunk changes before every frame, so the steady
    // state is exactly that and no reuse at all.
    assert!(
        parsed.lines > 0 && parsed.parsed >= SAMPLED_FRAMES as u64,
        "{} hunks were re-parsed over {} lines across {SAMPLED_FRAMES} frames, so \
         the visible hunk is not changing under the highlighter and this gate is \
         timing the core's frame path with the syntax parser missing",
        parsed.parsed,
        parsed.lines
    );

    // Non-vacuity. A frame that reused rather than recomputed would be a cheap
    // frame for a reason that is not the code, and a percentile diluted with
    // them would pass while saying nothing. One recompute per frame is the floor,
    // and the per-frame edit above is what makes that hold at any frame rate
    // rather than only on a machine fast enough to finish inside the margin.
    assert!(
        cost.computed >= SAMPLED_FRAMES as u64,
        "{} diffs were recomputed across {SAMPLED_FRAMES} frames, so frames were \
         reusing and this gate timed settled frames",
        cost.computed
    );

    // And the premise, checked rather than assumed: a file the viewport never
    // drew is still inside its margin now, so it was for the whole window, since
    // settledness only ever increases with time. Two diffs rather than one,
    // because the first recomputes on any stale fingerprint and only the second
    // can tell "still unsettled" from "settled and reusable". Without this the
    // gate would quietly weaken on a runner slow enough to outrun the margin: the
    // other ninety-nine files would settle, and a shell that fetched ahead would
    // find them reusable and cheap.
    let undrawn = FILES - 1;
    let probed = frame.stats();
    frame.diff(undrawn).expect("diff");
    frame.diff(undrawn).expect("diff");
    let probe = delta(probed, frame.stats());
    assert_eq!(
        probe.reused, 0,
        "a file the viewport never drew was reusable after {SAMPLED_FRAMES} \
         frames, so the bulk rewrite settled part-way through and the tail of \
         this window was not the event"
    );

    // And the screen has to have been full, for the reason the gate above gives.
    let view = app
        .view(&mut frame, &mut highlighter, &history, height)
        .expect("view");
    assert_eq!(
        view.rows.len(),
        height,
        "the body drew {} of {height} rows, so the frames above were not full \
         screens",
        view.rows.len()
    );

    // Which also settles what the probe above assumed. It is named `undrawn` and
    // nothing had checked that it was: a fixture whose files were short enough
    // for the viewport to reach index {undrawn} would have fingerprinted it every
    // frame, and the probe would be asking a question about the drawn path while
    // reading as though it asked about the untouched one.
    assert!(
        view.top.file + view.read <= undrawn,
        "the viewport drew files {}..{} of {FILES}, which reaches the file the \
         settle probe treats as never drawn",
        view.top.file,
        view.top.file + view.read
    );

    let p99 = frames.percentile(0.99).expect("samples");
    assert!(
        p99 <= budget(I9_FRAME),
        "I9: a frame inside the settle margin after every one of {FILES} files \
         was rewritten at once was {p99:?} p99, over the {:?} budget (p50 {:?}, \
         max {:?}, {} diffs recomputed, {} reused, {} bytes)",
        budget(I9_FRAME),
        frames.percentile(0.50).expect("samples"),
        frames.max().expect("samples"),
        cost.computed,
        cost.reused,
        cost.bytes
    );
}

/// The wide fixture's shape, and why these two numbers.
///
/// Twenty files of sixty lines, rewritten line for line, so one file is a single
/// hunk of **120 display rows**. Both numbers are chosen against the scroll
/// rather than against the diff:
///
/// * a notch is [`WHEEL_ROWS`] rows, so a file boundary arrives every 40 frames
///   and about **2.4%** of the samples enter a hunk nothing has parsed. At 250
///   samples a nearest-rank p99 is the third-worst frame, so the partition below
///   separates frames the percentile can actually reach rather than a tail
///   nothing occupies;
/// * twenty files is 2440 rows, comfortably more than the 900 a warmup and a
///   sample walk together, so neither direction runs into an end and starts
///   measuring a viewport that cannot move.
const WIDE_FILES: usize = 20;
const WIDE_LINES: usize = 60;

/// Display rows one wide file contributes: every line removed and every line
/// added.
const WIDE_HUNK_ROWS: usize = WIDE_LINES * 2;

/// Where the upward scroll starts, in rows from the top of the diff.
///
/// Far enough down that 300 frames of three rows never reach the top, since a
/// viewport pinned at row zero stops crossing boundaries and the gate would
/// quietly become a measurement of one file.
const UP_FROM: usize = 1_000;

/// Files that have to sit above the viewport before an upward scroll is one.
const UP_FILES: usize = 4;

/// What one scroll of a wide fixture cost, per stage and per partition.
struct Scrolled {
    /// Frames that reused every hunk they drew: the steady state I9 is about.
    warm: Samples,
    /// Frames that entered a hunk nothing had parsed: `SPEC.md` §7's cold path.
    cold: Samples,
    collect: Samples,
    paint: Samples,
    /// The worst single cold frame's parse, in lines.
    cold_lines: u64,
    /// Lines the whole run highlighted.
    lines: u64,
    /// Hunks the run swept out of the cache, which is the third suspect's own
    /// number: an eviction is only a cost when the reader comes back to it.
    evicted: u64,
    boundaries: usize,
    widest: usize,
    body_rows: usize,
    /// Rows the body had, carried from the run rather than re-derived.
    ///
    /// `hold_the_scroll_budget` writes two assertions in terms of it, and
    /// rebuilding it there from a fresh `App` would be a second source of truth
    /// for a number this run already has.
    height: usize,
    painted: PaintStats,
}

impl Scrolled {
    fn report(&self, what: &str) {
        eprintln!(
            "{what}: {} warm frames p50 {:?} p99 {:?} max {:?} | \
             {} cold frames p50 {:?} p99 {:?} max {:?} | \
             collect p99 {:?} paint p99 {:?} | \
             {} boundaries, {} lines highlighted, {} hunks evicted, \
             worst cold parse {} lines, \
             {} rows painted from {} characters, widest line {}",
            self.warm.len(),
            self.warm.percentile(0.50).unwrap_or_default(),
            self.warm.percentile(0.99).unwrap_or_default(),
            self.warm.max().unwrap_or_default(),
            self.cold.len(),
            self.cold.percentile(0.50).unwrap_or_default(),
            self.cold.percentile(0.99).unwrap_or_default(),
            self.cold.max().unwrap_or_default(),
            self.collect.percentile(0.99).unwrap_or_default(),
            self.paint.percentile(0.99).unwrap_or_default(),
            self.boundaries,
            self.lines,
            self.evicted,
            self.cold_lines,
            self.painted.rows,
            self.painted.examined,
            self.widest,
        );
    }
}

/// Frames in one leg of [`Motion::Back`], chosen to cross two files each way.
const LEG: usize = 80;

/// How the reader is moving.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Motion {
    Down,
    /// From [`UP_FROM`] rows in, upwards, into hunks nothing has parsed.
    Up,
    /// Down a couple of files, then back up over the same ground, repeatedly.
    ///
    /// The third suspect's own workload. [`Up`](Self::Up) measures a *first*
    /// entry, which no cache can make cheap; this measures a **second** one,
    /// which is the case `Highlighter::sweep` decides. A reader scrolling back
    /// to something they just passed is the ordinary motion, and if eviction on
    /// hunk exit is a real cost it is here that it shows.
    Back,
}

impl Motion {
    fn step(self, at: usize) -> isize {
        match self {
            Motion::Down => WHEEL_ROWS,
            Motion::Up => -WHEEL_ROWS,
            Motion::Back if (at / LEG) % 2 == 0 => WHEEL_ROWS,
            Motion::Back => -WHEEL_ROWS,
        }
    }
}

#[test]
fn scrolling_down_wide_lines_holds_the_frame_budget() {
    let Some(run) = scroll("wide-down", Motion::Down, WIDE_EXT) else {
        return;
    };
    hold_the_scroll_budget(&run, "scroll down");
}

#[test]
fn scrolling_up_wide_lines_holds_the_frame_budget() {
    // The direction `SPEC.md` §10 names as the worst case and which nothing had
    // ever run. Scrolling **down** enters a new hunk at its top, where the
    // forward-only parse has nothing above it to pay for; scrolling up enters
    // the same hunk at its **bottom**, so the first frame there parses the whole
    // file in order to draw its last rows.
    let Some(run) = scroll("wide-up", Motion::Up, WIDE_EXT) else {
        return;
    };
    hold_the_scroll_budget(&run, "scroll up");
}

#[test]
fn scrolling_back_over_ground_already_read_holds_the_frame_budget() {
    let Some(run) = scroll("wide-back", Motion::Back, WIDE_EXT) else {
        return;
    };
    hold_the_scroll_budget(&run, "scroll back");
}

#[test]
fn the_parse_is_attributed_by_subtracting_a_grammarless_run() {
    // The third suspect, given a number instead of a ranking. Two runs over
    // byte-identical content, one under a grammar and one under an extension
    // `syntect` has none for, so what separates them is the parse and nothing
    // else. Reported rather than gated: a difference of two wall clocks is
    // evidence, and `SPEC.md` §7 keeps the verdicts on named fixtures.
    //
    // What *is* asserted is the premise, because it is the half that can rot:
    // the grammarless run must really parse nothing.
    let Some(parsed) = scroll("wide-parse", Motion::Down, WIDE_EXT) else {
        return;
    };
    let Some(plain) = scroll("wide-plain", Motion::Down, WIDE_UNPARSED_EXT) else {
        return;
    };
    parsed.report("scroll down, with a grammar");
    plain.report("scroll down, grammarless");

    assert_eq!(
        plain.lines, 0,
        "the grammarless run highlighted {} lines, so `.{WIDE_UNPARSED_EXT}` is \
         no longer grammarless and this subtraction is between two parses",
        plain.lines
    );
    assert!(
        parsed.lines > 0,
        "the run under a grammar highlighted nothing, so there is no parse to \
         attribute"
    );

    let with = parsed.collect.percentile(0.99).unwrap_or_default();
    let without = plain.collect.percentile(0.99).unwrap_or_default();
    eprintln!(
        "the parse is {:?} of a scrolled frame's collect at p99 ({with:?} with a \
         grammar against {without:?} without), over {} lines",
        with.saturating_sub(without),
        parsed.lines,
    );
}

/// Scroll a wide-character fixture one notch a frame and measure it.
///
/// One function rather than several tests with a sign swapped, for the reason
/// [`frame_budget_at_depth`] gives: two runs only mean anything against each
/// other while every other term agrees.
///
/// **No edits, and no `Frame::advance`.** Every other gate in this file drives
/// the frame path with a writer in the other pane, which is I9's own wording.
/// This one drives it with the reader's own thumb, and that is a different frame:
/// `vigia::run` advances the frame on a `Wake::Tick` and *not* on a key or a
/// wheel event, so a scroll frame is collect plus paint. Timing an advance here
/// would add 2.7ms of work the product does not do on a keystroke and bury the
/// term being measured under it.
///
/// Returns `None` when the absolute tier does not apply, after the structural
/// setup has run.
fn scroll(name: &str, motion: Motion, ext: &str) -> Option<Scrolled> {
    let scratch = Scratch::wide_lines_as(name, WIDE_FILES, WIDE_LINES, ext);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    settle(&mut frame);
    assert_eq!(
        frame.files().len(),
        WIDE_FILES,
        "fixture is not {WIDE_FILES} files"
    );

    let mut app = App::new();
    let mut highlighter = Highlighter::new();
    let history = History::new();
    let height = body(&app, WIDE_FILES);

    if motion == Motion::Up {
        app.apply(
            Action::Scroll(isize::try_from(UP_FROM).expect("a sane depth")),
            &mut frame,
            height,
        )
        .expect("scroll");
        // Resolved by the collect rather than by the scroll: `App` adds the rows
        // to the current file's offset and lets `View::collect` carry the
        // overrun into the files below, which is what keeps a scroll to one diff
        // per file. So the position is asserted *after* a view, and in files
        // rather than in rows.
        let view = app
            .view(&mut frame, &mut highlighter, &history, height)
            .expect("view");
        assert!(
            view.top.file >= UP_FILES,
            "{UP_FROM} rows landed on file {} of {WIDE_FILES}, so there are \
             fewer than {UP_FILES} files above the viewport and scrolling up \
             will reach the top before it has crossed anything",
            view.top.file
        );
    }

    if !absolute_gates_apply() {
        return None;
    }

    let _timed = exclusively_timed();

    let theme = Theme::default();
    let mut buf = Buffer::empty(area());

    // Split rather than timed whole, because "which of the three dominates" is
    // what #45 asks and one total cannot answer it. Their sum is what the budget
    // is held against, so nothing is counted twice.
    let mut run = Scrolled {
        warm: Samples::new(SAMPLED_FRAMES),
        cold: Samples::new(SAMPLED_FRAMES),
        collect: Samples::new(SAMPLED_FRAMES),
        paint: Samples::new(SAMPLED_FRAMES),
        cold_lines: 0,
        lines: 0,
        evicted: 0,
        boundaries: 0,
        widest: 0,
        body_rows: 0,
        height,
        painted: PaintStats::default(),
    };
    let mut at_file = usize::MAX;

    for at in 0..(WARMUP_FRAMES + SAMPLED_FRAMES) {
        app.apply(Action::Scroll(motion.step(at)), &mut frame, height)
            .expect("scroll");

        let before = highlighter.stats();
        let (screen, collect) = timed(|| {
            app.view(&mut frame, &mut highlighter, &history, height)
                .expect("view")
        });
        let chrome = app.chrome("fixture", None);
        let (painted, paint) = timed(|| render(&mut buf, area(), &screen, &theme, &chrome));
        let parsed = highlight_delta(before, highlighter.stats());

        if screen.top.file != at_file {
            at_file = screen.top.file;
            run.boundaries += 1;
        }
        run.body_rows = screen.rows.len();
        run.widest = run.widest.max(
            screen
                .rows
                .iter()
                .filter_map(|row| match row {
                    Row::Line { text, .. } => Some(text.chars().count()),
                    _ => None,
                })
                .max()
                .unwrap_or(0),
        );

        if at < WARMUP_FRAMES {
            continue;
        }

        run.lines += parsed.lines;
        run.evicted += parsed.evicted;
        // Accumulated, like the two above it and unlike the assignment this used
        // to be. `Scrolled::report` prints all three in one sentence, so a field
        // holding the last frame while its neighbours hold the run was a figure
        // that read as a total and was not one.
        run.painted += painted;
        run.collect.push(collect);
        run.paint.push(paint);

        // The partition, and it is `SPEC.md` §7's carve-out rather than a
        // convenience: a frame that parses a hunk for the first time is on the
        // cold path, which I9 excludes by definition. Diluting the steady state
        // with those would let a regression hide behind them, and dropping them
        // silently would hide the one number #45 exists to surface. So they are
        // separated, both are reported, and only the steady half is asserted.
        if parsed.parsed > 0 {
            run.cold.push(collect + paint);
            run.cold_lines = run.cold_lines.max(parsed.lines);
        } else {
            run.warm.push(collect + paint);
        }
    }

    Some(run)
}

/// The assertions both directions share.
fn hold_the_scroll_budget(run: &Scrolled, what: &str) {
    run.report(what);

    let height = run.height;

    // Non-vacuity, in four directions.
    //
    // The screen has to have been full, or a frame that drew two rows is a cheap
    // frame for a reason that is not the code.
    assert_eq!(
        run.body_rows, height,
        "the last body drew {} of {height} rows, so these were not full screens",
        run.body_rows
    );

    // Boundaries have to have been crossed, or this measured one file and the
    // cold half of the partition is empty for a reason that is the fixture
    // rather than the code.
    assert!(
        run.boundaries >= UP_FILES,
        "the viewport crossed {} file boundaries in {} frames, so this run never \
         entered a hunk it had not parsed",
        run.boundaries,
        WARMUP_FRAMES + SAMPLED_FRAMES
    );
    assert!(
        !run.cold.is_empty() && !run.warm.is_empty(),
        "the partition is one-sided: {} warm frames and {} cold",
        run.warm.len(),
        run.cold.len()
    );

    // Highlighting has to have actually happened, which is the direction this
    // whole file exists for and the one the partition above cannot see: a run
    // over a file type nothing recognises has warm frames, cold frames and
    // boundaries, and is the core's frame path with the syntax parser missing.
    assert!(
        run.lines > 0,
        "no lines were highlighted across the sampled frames, so this gate is \
         timing a collect with the parser idle"
    );

    // The lines have to be wider than the pane, or this is `large_diff` with a
    // different name on it and it cannot tell a bounded paint from an unbounded
    // one.
    assert!(
        run.widest > usize::from(area().width),
        "the widest drawn line is {} characters against an {}-column pane, so \
         this fixture never exceeds the pane",
        run.widest,
        area().width
    );

    // And a cold frame has to be bounded by the hunk it entered, which is what
    // says the rewind still holds at this width: one whole new hunk, plus a
    // screenful of the neighbour beside it, plus the stride a changed hunk can
    // rewind past.
    let cold_bound = (WIDE_HUNK_ROWS + height + CHECKPOINT_STRIDE) as u64;
    assert!(
        run.cold_lines <= cold_bound,
        "a frame entering a new hunk parsed {} lines, over the {cold_bound} that \
         one hunk plus a screen can cost, so the parse is not bounded by what \
         the frame entered",
        run.cold_lines
    );

    let p99 = run.warm.percentile(0.99).expect("samples");
    assert!(
        p99 <= budget(I9_FRAME),
        "I9: {what} through wide lines was {p99:?} p99 over {} steady frames, \
         past the {:?} budget (p50 {:?}, max {:?}; collect p99 {:?}, paint p99 \
         {:?}; {} cold frames at {:?} p99)",
        run.warm.len(),
        budget(I9_FRAME),
        run.warm.percentile(0.50).expect("samples"),
        run.warm.max().expect("samples"),
        run.collect.percentile(0.99).unwrap_or_default(),
        run.paint.percentile(0.99).unwrap_or_default(),
        run.cold.len(),
        run.cold.percentile(0.99).unwrap_or_default(),
    );
}
