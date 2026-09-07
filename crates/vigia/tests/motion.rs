//! `SPEC.md` §11.1's movements, composed: what a motion is worth as a length.
//!
//! The effects themselves are `arriving.rs`'s subject. What is here is the
//! arithmetic a caller would otherwise do by hand, because a composed motion is
//! retired by a clock as well as by its own count: told a length shorter than
//! it draws, a departure's rows are dropped mid-sweep, and told a longer one it
//! holds the frame clock after there is nothing left to draw.

use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use vigia::{
    ARRIVING_FRAME, LEAVING, Motion, RESOLVE_ARRIVING, RESOLVE_BEAT, RESOLVED_DEPARTURE, SWEEP,
    TRANSITION, Theme, effect_interval,
};

/// The pane the gates here draw on.
const PANE: Rect = Rect::new(0, 0, 40, 4);

/// A buffer with something in every cell, so an effect that changes nothing is
/// visible as a buffer that is unchanged.
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

#[test]
fn a_sequence_is_as_long_as_its_parts_together() {
    // The resolve's departure, composed the way `notes.rs` composes it. Its
    // three parts are what `RESOLVED_DEPARTURE` is defined as, and the ledger
    // drops the rows at that moment: a motion that reported anything else would
    // leave them drawn after the sweep or take them away during it.
    let departure = Motion::evolve(RESOLVE_ARRIVING)
        .hold(RESOLVE_BEAT)
        .then(Motion::sweep(LEAVING));
    assert_eq!(departure.length(), RESOLVED_DEPARTURE);
    assert_eq!(
        departure.length(),
        RESOLVE_ARRIVING + RESOLVE_BEAT + LEAVING,
        "a sequence reported something other than its parts added up"
    );
}

#[test]
fn a_pair_at_once_is_as_long_as_the_slower_one() {
    let slow = Motion::evolve(RESOLVE_ARRIVING).with(Motion::coalesce(ARRIVING_FRAME));
    assert_eq!(
        slow.length(),
        RESOLVE_ARRIVING,
        "two motions at once reported the first's length rather than the longer"
    );
    // And the other way round, or the rule is a coincidence of the order.
    let same = Motion::coalesce(ARRIVING_FRAME).with(Motion::evolve(RESOLVE_ARRIVING));
    assert_eq!(same.length(), RESOLVE_ARRIVING);
}

#[test]
fn a_motion_is_retired_by_its_own_length() {
    let now = Instant::now();
    let armed = Motion::sweep(LEAVING).across(SWEEP).armed(now);
    assert!(
        !armed.spent(now + LEAVING - Duration::from_millis(1)),
        "a motion retired a millisecond before its length was up"
    );
    assert!(
        armed.spent(now + LEAVING),
        "a motion outlived its own length, so the cells under it are never released"
    );
}

#[test]
fn a_motion_armed_after_a_quiet_spell_starts_at_its_beginning() {
    // The rule the loop owes every motion: the interval since the previous
    // paint is the whole of a quiet spell, and a motion armed on the wake that
    // ended it has lived through none of that.
    let spell = Duration::from_secs(600);
    assert_eq!(effect_interval(false, spell), Duration::ZERO);
    assert_eq!(effect_interval(true, ARRIVING_FRAME), ARRIVING_FRAME);

    // And what it prevents, on a composed motion: told the spell, the whole
    // departure is over inside one frame and the reader sees none of it.
    let mut departure = Motion::evolve(RESOLVE_ARRIVING)
        .hold(RESOLVE_BEAT)
        .then(Motion::sweep(LEAVING))
        .effect();
    let mut buf = drawn();
    departure.process(spell.into(), &mut buf, PANE);
    assert!(
        departure.done(),
        "a departure survived an interval longer than itself, so the rule above \
         guards nothing"
    );
}

#[test]
fn an_evolve_draws_where_a_crossfade_has_no_two_inks_to_travel_between() {
    // The reason this surface evolves rather than fades: a crossfade needs a
    // colour to travel from, and a palette with no colour at all has none, so
    // an arrival written as one is not drawn where the depth has flattened it.
    // Shade blocks are glyphs, so they arrive at every depth.
    let flat = Theme::ansi();
    let mut evolve = Motion::evolve(RESOLVE_ARRIVING)
        .ink(flat.chrome_dim)
        .from_centre(TRANSITION)
        .effect();
    let settled = drawn();
    let mut buf = drawn();
    evolve.process(Duration::from_millis(200).into(), &mut buf, PANE);
    assert_ne!(
        buf, settled,
        "a third of the way in the evolve has changed no cell"
    );
    let shades = ["░", "▒", "▓", "█"];
    let shading = (0..PANE.height)
        .flat_map(|y| (0..PANE.width).map(move |x| (x, y)))
        .filter(|(x, y)| shades.contains(&buf[(*x, *y)].symbol()))
        .count();
    assert!(shading > 0, "the evolve drew no shade block:\n{buf:?}");

    // On the frame the renderer drew next, as the pane paints it: an effect
    // that is done leaves the buffer it is handed alone.
    let mut buf = drawn();
    evolve.process(RESOLVE_ARRIVING.into(), &mut buf, PANE);
    assert!(evolve.done());
    assert_eq!(
        buf, settled,
        "the evolve did not settle on what the renderer drew beneath it"
    );
}
