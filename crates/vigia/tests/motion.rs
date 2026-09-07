//! `SPEC.md` §11.1's movements, written in `tachyonfx`'s DSL.
//!
//! The sources are text, so a typo in one is a runtime failure rather than a
//! build one, and the pane's answer to a source that will not compile is to
//! draw nothing. That is only safe because every source this repository ships
//! is compiled here: this file is what stands between the two.

use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use tachyonfx::pattern::{AnyPattern, SweepPattern};
use vigia::motion::{
    self, ARRIVING, ARRIVING_FRAME, LEAVING, RESOLVE_ARRIVING, RESOLVE_BEAT, RESOLVED_DEPARTURE,
    Timed, effect_interval, length,
};

/// The pane the gates here draw on.
const PANE: Rect = Rect::new(0, 0, 40, 4);

/// The shade blocks an arriving surface evolves through.
const SHADES: [&str; 4] = ["░", "▒", "▓", "█"];

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

/// Every motion this pane ships, built the way the pane builds it: its name,
/// the effect, the length it must run for, and a moment it must be drawing at.
///
/// The moment is named per motion rather than taken as half the length, because
/// a departure spends its middle holding still on purpose.
fn every_motion() -> Vec<(&'static str, tachyonfx::Effect, Duration, Duration)> {
    let ink = Style::default().fg(Color::Cyan);
    vec![
        (
            "evolving",
            motion::evolving(ink, RESOLVE_ARRIVING, 10.0),
            RESOLVE_ARRIVING,
            RESOLVE_ARRIVING / 2,
        ),
        (
            "sweeping",
            motion::sweeping(LEAVING, 35),
            LEAVING,
            LEAVING / 2,
        ),
        (
            "holding",
            motion::holding(
                motion::evolving(ink, RESOLVE_ARRIVING, 10.0),
                RESOLVE_BEAT,
                motion::sweeping(LEAVING, 35),
            ),
            RESOLVED_DEPARTURE,
            RESOLVE_ARRIVING / 2,
        ),
        (
            "fading in",
            motion::fading(Color::Blue, ARRIVING, AnyPattern::default(), false),
            ARRIVING,
            ARRIVING / 2,
        ),
        (
            "fading out",
            motion::fading(
                Color::Blue,
                ARRIVING,
                SweepPattern::right_to_left(12).into(),
                true,
            ),
            ARRIVING,
            ARRIVING / 2,
        ),
        (
            "coalescing",
            motion::coalescing(ARRIVING),
            ARRIVING,
            ARRIVING / 2,
        ),
    ]
}

#[test]
fn every_source_compiles_and_runs_for_the_length_it_was_given() {
    // A source that does not compile draws nothing, which is the right answer
    // on a monitor and the wrong one to find out about from a reader. Both
    // halves are checked: the effect is not the empty stand-in, and it runs for
    // the duration the pane armed it for.
    for (name, mut effect, want, when) in every_motion() {
        assert_eq!(
            length(&effect),
            want,
            "{name} does not run for the length it was given, so its rows are              dropped early or held after it ends"
        );
        let settled = drawn();
        let mut buf = drawn();
        effect.process(when.into(), &mut buf, PANE);
        assert_ne!(
            buf, settled,
            "{name} changed no cell {when:?} in, so its source compiled to              nothing a reader can see"
        );
    }
}

#[test]
fn a_departure_is_as_long_as_its_three_parts() {
    // The ledger drops a resolved note's rows at `RESOLVED_DEPARTURE`, so the
    // effect over them has to end at the same moment: earlier and the rows sit
    // blank, later and they are taken away mid-sweep.
    let ink = Style::default().fg(Color::Cyan);
    let departure = motion::holding(
        motion::evolving(ink, RESOLVE_ARRIVING, 10.0),
        RESOLVE_BEAT,
        motion::sweeping(LEAVING, 35),
    );
    assert_eq!(length(&departure), RESOLVED_DEPARTURE);
    assert_eq!(
        length(&departure),
        RESOLVE_ARRIVING + RESOLVE_BEAT + LEAVING
    );
}

#[test]
fn a_motion_is_retired_by_the_length_it_reports() {
    let now = Instant::now();
    let armed = Timed::armed(motion::sweeping(LEAVING, 35), now);
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
    let ink = Style::default().fg(Color::Cyan);
    let mut departure = motion::holding(
        motion::evolving(ink, RESOLVE_ARRIVING, 10.0),
        RESOLVE_BEAT,
        motion::sweeping(LEAVING, 35),
    );
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
    let flat = vigia::Theme::ansi();
    let mut evolve = motion::evolving(flat.chrome_dim, RESOLVE_ARRIVING, 10.0);
    let settled = drawn();
    let mut buf = drawn();
    evolve.process(Duration::from_millis(200).into(), &mut buf, PANE);
    let shading = (0..PANE.height)
        .flat_map(|y| (0..PANE.width).map(move |x| (x, y)))
        .filter(|(x, y)| SHADES.contains(&buf[(*x, *y)].symbol()))
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
