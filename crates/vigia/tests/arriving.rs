//! `SPEC.md` §5.3: a change may be drawn arriving, and §6's `tachyonfx` row.
//!
//! The gate this file exists for is the one the hand-rolled attempt did not have:
//! that the cells actually change. That version passed every assertion written
//! about it and drew nothing a reader could see, because every assertion was about
//! the arithmetic and none was about the buffer.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use tachyonfx::{Duration as FxDuration, EffectManager, Interpolation, fx};
use vigia::{ARRIVING, ARRIVING_FRAME};

/// The pane every gate here draws on.
const PANE: Rect = Rect::new(0, 0, 80, 24);

/// A buffer with text in it, standing in for what `render` leaves behind.
fn drawn() -> Buffer {
    let mut buf = Buffer::empty(PANE);
    for row in 0..PANE.height {
        buf.set_string(
            0,
            row,
            "src/engine.rs  +42 -7",
            ratatui::style::Style::default(),
        );
    }
    buf
}

/// Every cell's symbol, so two buffers can be compared as what a reader sees.
fn symbols(buf: &Buffer) -> Vec<String> {
    (0..PANE.height)
        .flat_map(|y| (0..PANE.width).map(move |x| (x, y)))
        .map(|(x, y)| buf[(x, y)].symbol().to_owned())
        .collect()
}

#[test]
fn an_arriving_effect_changes_the_cells_it_covers() {
    // The whole point. An effect that runs and leaves the buffer identical is the
    // failure this file exists to catch, and it is not visible from the arithmetic.
    let settled = drawn();
    let mut buf = drawn();
    let mut effects: EffectManager<String> = EffectManager::default();
    effects.add_unique_effect(
        "src/engine.rs".to_owned(),
        fx::coalesce((FxDuration::from(ARRIVING), Interpolation::QuadOut)),
    );

    // One frame in, which is where a reader's eye actually is.
    effects.process_effects(FxDuration::from(ARRIVING_FRAME), &mut buf, PANE);
    assert_ne!(
        symbols(&buf),
        symbols(&settled),
        "the effect ran and left every cell exactly as it found it, so nothing \
         reaches the pane"
    );
}

#[test]
fn an_arriving_effect_settles_on_what_was_drawn_beneath_it() {
    // It has to end where the ordinary render ends, or a row keeps a shape the
    // renderer never gives it.
    let settled = drawn();
    let mut buf = drawn();
    let mut effects: EffectManager<String> = EffectManager::default();
    effects.add_unique_effect(
        "src/engine.rs".to_owned(),
        fx::coalesce((FxDuration::from(ARRIVING), Interpolation::QuadOut)),
    );

    // One frame at a time, as the loop runs it: the widgets redraw the buffer and
    // the effect works on what they left. A harness that skipped the redraw would
    // accumulate the effect's own output and prove nothing about where it settles.
    let mut spent = std::time::Duration::ZERO;
    while spent < ARRIVING * 2 {
        buf = drawn();
        effects.process_effects(FxDuration::from(ARRIVING_FRAME), &mut buf, PANE);
        spent += ARRIVING_FRAME;
    }

    assert!(
        !effects.is_running(),
        "the effect is still running past twice its own length, so it would hold \
         the clock for good"
    );
    assert_eq!(
        symbols(&buf),
        symbols(&settled),
        "the effect finished on cells the renderer never drew"
    );
}

#[test]
fn a_finished_effect_stops_asking_for_frames() {
    // I1's bound, on the object that owns it. `Shell::patience` folds exactly this
    // answer, and `input.rs` pins the fold; what this pins is the answer itself.
    let mut effects: EffectManager<String> = EffectManager::default();
    assert!(
        !effects.is_running(),
        "an empty manager reports itself running, so a pane with no effect is timed"
    );

    effects.add_unique_effect(
        "src/engine.rs".to_owned(),
        fx::coalesce((FxDuration::from(ARRIVING), Interpolation::QuadOut)),
    );
    assert!(
        effects.is_running(),
        "an armed effect does not report itself running"
    );

    let mut buf;
    let mut spent = std::time::Duration::ZERO;
    while spent < ARRIVING * 2 {
        buf = drawn();
        effects.process_effects(FxDuration::from(ARRIVING_FRAME), &mut buf, PANE);
        spent += ARRIVING_FRAME;
    }
    assert!(
        !effects.is_running(),
        "the effect never gives the clock back, so the idle path is timed for good"
    );
}

#[test]
fn a_second_write_to_one_file_replaces_its_effect_rather_than_stacking() {
    // Keyed by path, because an agent saving the same file repeatedly is the
    // ordinary workload rather than an edge, and effects that pile up on one row
    // would each be drawing over the last.
    let mut effects: EffectManager<String> = EffectManager::default();
    let mut buf;
    for _ in 0..8 {
        effects.add_unique_effect(
            "src/engine.rs".to_owned(),
            fx::coalesce((FxDuration::from(ARRIVING), Interpolation::QuadOut)),
        );
        buf = drawn();
        effects.process_effects(FxDuration::from(ARRIVING_FRAME), &mut buf, PANE);
    }

    // Eight arms, and the whole lot still ends inside one effect's length.
    let mut spent = std::time::Duration::ZERO;
    while spent < ARRIVING * 2 {
        buf = drawn();
        effects.process_effects(FxDuration::from(ARRIVING_FRAME), &mut buf, PANE);
        spent += ARRIVING_FRAME;
    }
    assert!(
        !effects.is_running(),
        "repeated writes to one file stacked their effects, so the last one to \
         finish is holding the clock for all of them"
    );
}

#[test]
fn an_effect_is_bounded_by_the_pulse_rung_it_decays_into() {
    // `Recency::Pulse` is guaranteed a whole sample, so an effect shorter than one
    // always ends while the grid still says *Pulse* and the two cannot disagree
    // about how long ago *now* was, which is what §5.1 refuses.
    assert!(
        ARRIVING < vigia_core::HISTORY_SAMPLE,
        "an effect of {ARRIVING:?} outlives the {:?} the pulse rung is guaranteed",
        vigia_core::HISTORY_SAMPLE
    );
    assert!(
        ARRIVING_FRAME < ARRIVING,
        "a frame of the effect is not shorter than the effect, so it draws once"
    );
}

#[test]
fn the_effect_reports_its_own_completion_rather_than_a_clock_we_keep() {
    // Why the `App`-side mirror of this was deleted: the library already answers
    // it, and two answers to one question is the shape that drifts.
    let mut effect = fx::coalesce((FxDuration::from(ARRIVING), Interpolation::QuadOut));
    assert!(
        !effect.done(),
        "a fresh effect reports itself already finished"
    );

    let mut buf = drawn();
    effect.process(FxDuration::from(ARRIVING), &mut buf, PANE);
    assert!(
        effect.done(),
        "an effect run for its whole length does not report itself finished, so \
         nothing would ever release the clock"
    );
}
