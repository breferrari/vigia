//! `SPEC.md` §5.3: a change may be drawn arriving. What that costs and what it
//! draws, reported rather than gated: the numbers here are the deliverable and
//! the judgement they serve is the reader's.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use std::time::{Duration, Instant};

use ratatui::layout::Rect;
use ratatui::style::Color;
use vigia::{
    ARRIVING, ARRIVING_FRAME, ARRIVING_STEPS, Action, App, Chrome, Deadlines, Depth, Glyphs,
    Pointing, Theme, patience,
};
use vigia_core::{HISTORY_SAMPLE, History, Recency};

/// The coalescer's own bound on how often a tick can arrive, from
/// `vigia-core/src/watch.rs`. A fade re-armed by every tick is re-armed this often.
const MAX_DELAY: Duration = Duration::from_millis(100);

#[test]
fn a_fade_finishes_inside_the_pulse_it_decays_into() {
    // The bound that keeps §5.1's two clocks from disagreeing about how long ago
    // *now* was: `Recency::Pulse` is guaranteed a whole sample, so a fade shorter
    // than one always ends while the grid still says *Pulse*. This is the reason
    // `ARRIVING` is a constant under `HISTORY_SAMPLE` rather than a taste.
    assert!(
        ARRIVING < HISTORY_SAMPLE,
        "a fade of {ARRIVING:?} outlives the {HISTORY_SAMPLE:?} the pulse rung is \
         guaranteed, so the ink and the grid would disagree"
    );
}

#[test]
fn the_fade_is_drawn_and_lands_on_the_rung_it_decays_into() {
    // The interpolation is between two inks the palette already has: the pulse
    // mark's, which means *when*, and the rung the row settles on. On the truecolor
    // palette, because that is the only depth with a fourth intensity to spend.
    let theme = Theme::dark().resolve(Depth::Truecolor);
    let settled = theme.recency(Recency::Pulse);
    let first = theme.arriving(Recency::Pulse, 0);
    let last = theme.arriving(Recency::Pulse, ARRIVING_STEPS);

    // Non-vacuity first: a palette whose two endpoints match would make every
    // assertion below trivially true.
    assert_ne!(
        first.fg, settled.fg,
        "the fade's first step already draws the settled ink, so there is no fade          here and nothing below this can fail"
    );
    assert_eq!(
        last.fg, settled.fg,
        "the fade does not land on the rung it decays into, so a row would keep a          colour the ladder never gives it"
    );
    // And it moves monotonically between them rather than jumping about, which is
    // what makes it read as one motion.
    let reds: Vec<u8> = (0..=ARRIVING_STEPS)
        .filter_map(|step| match theme.arriving(Recency::Pulse, step).fg {
            Some(Color::Rgb(r, _, _)) => Some(r),
            _ => None,
        })
        .collect();
    assert_eq!(
        reds.len(),
        usize::from(ARRIVING_STEPS) + 1,
        "some step of the fade is not truecolor, so the ramp has a hole in it"
    );
    let rising = reds.windows(2).all(|pair| pair[0] <= pair[1]);
    let falling = reds.windows(2).all(|pair| pair[0] >= pair[1]);
    assert!(
        rising || falling,
        "the fade's ramp is not monotone: {reds:?}"
    );

    println!("--- #365: the ramp the fade draws, truecolor palette");
    println!("fade starts at                 {:?}", first.fg);
    println!("settles on                     {:?}", settled.fg);
    println!("ramp, red channel              {reds:?}");
}

/// Wakes the loop is handed over one burst and its tail, counted by driving
/// `patience` exactly as `run` does.
fn wakes_for_one_burst(app: &App, from: Instant) -> usize {
    let mut now = from;
    let mut wakes = 0;
    // Bounded so a fade that never expires fails this rather than hanging it.
    while let Some(wait) = patience(
        Deadlines {
            arriving: app.arriving_until(now),
            ..Deadlines::default()
        },
        now,
    ) {
        now += wait.max(Duration::from_nanos(1));
        wakes += 1;
        assert!(wakes < 10_000, "the fade never gave the clock back");
    }
    wakes
}

#[test]
fn a_fade_costs_one_wake_per_burst_and_none_when_the_tree_is_quiet() {
    // The claim the whole design rests on: the fade renders on the frames the
    // writes were already causing, and asks for a clock only to finish the one
    // whose arming tick turned out to be the last.
    let now = Instant::now();

    let quiet = App::new();
    assert_eq!(
        wakes_for_one_burst(&quiet, now),
        0,
        "a shell that has seen no tick is on a clock, so an idle pane is timed"
    );

    let mut app = App::new();
    app.arrived(now);
    let armed = wakes_for_one_burst(&app, now);
    let want = ARRIVING.as_millis() / ARRIVING_FRAME.as_millis();
    // The frames that fill the fade, plus the one that closes it.
    assert!(
        (armed as u128) <= want + 1,
        "a fade asked the loop for {armed} wakes where {want} frames of \
         {ARRIVING_FRAME:?} fill {ARRIVING:?}, so it is running past its own end"
    );
    assert!(
        armed > 1,
        "a fade asked for {armed} wake(s), so nothing draws its middle and a quiet \
         tree sees a flash rather than a fade"
    );

    println!("--- what a fade costs");
    println!("fade length                    {ARRIVING:?}");
    println!("pulse rung guaranteed for      {HISTORY_SAMPLE:?}");
    println!("frames it asks for per burst   {armed}, one every {ARRIVING_FRAME:?}");
    println!(
        "ticks under a writing agent    up to 1 every {MAX_DELAY:?}, so a fade is \
         re-armed before it ends and the cadence is continuous while writing"
    );
    let per_second = 1000 / ARRIVING_FRAME.as_millis();
    println!(
        "cost while an agent writes     {per_second} frames a second, against the {} \
         the loop draws from ticks alone",
        1000 / MAX_DELAY.as_millis()
    );
    println!(
        "cost on a quiet tree           {armed} frames once, then untimed: the fade \
         cannot re-arm itself and nothing else is waking the loop"
    );
}

#[test]
fn the_cadence_is_what_buys_the_ramp_and_the_price_is_the_frames() {
    // The trade, reported rather than judged, and §11.1 named its failure before
    // this existed: *a fraction of it read on a quiet tree is a number that ages
    // without being redrawn*. Without a cadence a fade renders only on the frames
    // the writes happened to cause, which on a quiet tree is two, so a reader sees
    // a flash. With one it renders its whole ramp and asks for the frames to do it.
    let now = Instant::now();
    let mut app = App::new();
    app.arrived(now);

    let sample = |every: Duration| {
        let mut steps = Vec::new();
        let mut at = now;
        while at < now + ARRIVING {
            if let Some(step) = app.arriving_step(at) {
                steps.push(step);
            }
            at += every;
        }
        steps
    };
    let on_ticks = sample(MAX_DELAY);
    let on_cadence = sample(ARRIVING_FRAME);

    println!("--- the ramp, and what it costs to draw it");
    println!("on ticks alone                 {on_ticks:?}");
    println!(
        "on the fade's own cadence      {} steps of 0..={ARRIVING_STEPS}",
        on_cadence.len()
    );
    println!(
        "the ladder it has to beat      3 rungs (pulse, live, cold), which cost no \
         clock at all"
    );

    assert!(
        on_cadence.len() > on_ticks.len(),
        "the cadence draws {} steps against the {} ticks alone give, so it is being \
         paid for and buying nothing",
        on_cadence.len(),
        on_ticks.len()
    );
    // And it is a ramp rather than a jump: every step differs from the one before.
    assert!(
        on_cadence.windows(2).all(|pair| pair[0] <= pair[1]),
        "the fade goes backwards: {on_cadence:?}"
    );
}

#[test]
fn a_change_arriving_draws_a_different_ink_from_one_that_has_settled() {
    // Read out of a painted pane rather than from the arithmetic, because the
    // first question is about what reaches the eye rather than what it costs.
    let scratch = support::Scratch::new("arriving-ink");
    scratch.write("src/a.rs", "seed\n");
    scratch.commit_all("base");
    scratch.write("src/a.rs", "changed\n");

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    support::materialise(&mut frame);
    let mut highlighter = vigia_core::Highlighter::eager();
    let mut history = History::new();
    let now = Instant::now();
    history.record_sized([("src/a.rs", Some(8u64))], now);

    let pane = Rect::new(0, 0, 80, 24);
    let mut app = App::new();
    let base: Chrome = app.chrome("fixture", None, Pointing::default(), 0, "");

    let row_of = |arriving: Option<u8>,
                  app: &mut App,
                  frame: &mut vigia_core::Frame,
                  highlighter: &mut vigia_core::Highlighter| {
        let mut chrome = base.clone();
        chrome.arriving = arriving;
        let laid = vigia::body_layout(pane, &chrome, 1, 1);
        let view = app
            .view(frame, highlighter, &history, laid)
            .expect("a view of the fixture");
        let mut buf = ratatui::buffer::Buffer::empty(pane);
        vigia::render(
            &mut buf,
            pane,
            &view,
            &Theme::dark().resolve(Depth::Truecolor),
            Glyphs::default(),
            &chrome,
        );
        let regions = vigia::regions(pane, &chrome, &view);
        // Every truecolor ink on the heading row, in the order drawn. Picking one
        // cell is what made an earlier form of this read `Reset` and prove nothing.
        let mut inks: Vec<Color> = Vec::new();
        for x in 0..pane.width {
            if let Some(Color::Rgb(r, g, b)) = buf[(x, regions.diff.top)].style().fg {
                let ink = Color::Rgb(r, g, b);
                if inks.last() != Some(&ink) {
                    inks.push(ink);
                }
            }
        }
        inks
    };

    let settled = row_of(None, &mut app, &mut frame, &mut highlighter);
    let steps = [0u8, 4, 8, 12, ARRIVING_STEPS];
    let drawn: Vec<Vec<Color>> = steps
        .iter()
        .map(|step| row_of(Some(*step), &mut app, &mut frame, &mut highlighter))
        .collect();

    println!("--- #365: the heading's inks through one fade, truecolor palette");
    println!("settled (no fade)              {settled:?}");
    for (step, inks) in steps.iter().zip(&drawn) {
        println!("step {step:>2} of {ARRIVING_STEPS}                   {inks:?}");
    }

    // Non-vacuity: the row has to be drawing truecolor at all, or every comparison
    // below is between two empty lists.
    assert!(
        !settled.is_empty(),
        "the heading drew no truecolor ink, so this readout cannot see a fade"
    );
    assert_ne!(
        drawn[0], settled,
        "the first step of the fade draws exactly the settled row, so nothing about          an arriving change reaches the pane"
    );
    assert_eq!(
        drawn.last().expect("a last step"),
        &settled,
        "the fade's last step does not match the settled row, so a reader would be          left on an ink the ladder never gives"
    );
}

#[test]
fn nothing_but_a_tick_arms_the_fade() {
    // I1's first condition, which no gate over the drawn pane can see: the fade is
    // armed by `App::arrived` and `App::arrived` is called from the tick arm alone.
    let now = Instant::now();
    let mut app = App::new();
    let scratch = support::Scratch::new("arriving-arming");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();

    for action in [
        Action::ToggleWrap,
        Action::ToggleFollow,
        Action::Scroll(1),
        Action::Page(1),
        Action::ToggleMasthead,
        Action::Redraw,
    ] {
        app.apply(action, &mut frame, 20).expect("apply");
        assert_eq!(
            app.arriving_until(now),
            None,
            "{action:?} armed the fade, so an act the reader performed is being \
             eased, which §5.3 still refuses"
        );
    }
}
