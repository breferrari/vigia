//! `SPEC.md` §5.3: a change may be drawn arriving, and §6's `tachyonfx` row.
//!
//! The gate this file exists for is the one the hand-rolled attempt did not have:
//! that the cells actually change. That version passed every assertion written
//! about it and drew nothing a reader could see, because every assertion was about
//! the arithmetic and none was about the buffer.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use tachyonfx::{Duration as FxDuration, EffectManager, Interpolation, fx};
use vigia::{
    ARRIVING, ARRIVING_FRAME, BOX_ARRIVING, LEAVING, RESOLVE_ARRIVING, RESOLVE_BEAT,
    RESOLVED_DEPARTURE, Theme, box_entrance, box_exit, effect_interval, leaving, resolve_arrival,
};

/// The pane every gate here draws on.
const PANE: Rect = Rect::new(0, 0, 80, 24);

/// Where the box's gates draw it: three rows under a line, across the content.
const BOX: Rect = Rect::new(6, 5, 60, 4);

/// A buffer with the box drawn in the chrome's ink, standing in for what
/// `render` leaves behind: a ring of rule and text inside it.
fn boxed(theme: &Theme) -> Buffer {
    let mut buf = Buffer::empty(PANE);
    for y in BOX.top()..BOX.bottom() {
        for x in BOX.left()..BOX.right() {
            let edge =
                y == BOX.top() || y + 1 == BOX.bottom() || x == BOX.left() || x + 1 == BOX.right();
            let cell = &mut buf[(x, y)];
            if edge {
                cell.set_symbol("─").set_style(theme.chrome_dim);
            } else {
                cell.set_symbol("x").set_style(theme.chrome);
            }
        }
    }
    buf
}

/// The ring's cells still drawn, and the inner cells still in the chrome's ink.
fn drawn_of(buf: &Buffer, theme: &Theme) -> (usize, usize) {
    let mut ring = 0;
    let mut lit = 0;
    for y in BOX.top()..BOX.bottom() {
        for x in BOX.left()..BOX.right() {
            let edge =
                y == BOX.top() || y + 1 == BOX.bottom() || x == BOX.left() || x + 1 == BOX.right();
            let cell = &buf[(x, y)];
            if edge {
                ring += usize::from(cell.symbol() != " ");
            } else {
                lit += usize::from(cell.style().fg == theme.chrome.fg);
            }
        }
    }
    (ring, lit)
}

/// The shade blocks an evolve draws on its way in.
const SHADES: [&str; 4] = ["░", "▒", "▓", "█"];

/// Cells inside the box holding a shade block rather than what the renderer left.
fn shading(buf: &Buffer) -> usize {
    let mut shaded = 0;
    for y in BOX.top()..BOX.bottom() {
        for x in BOX.left()..BOX.right() {
            shaded += usize::from(SHADES.contains(&buf[(x, y)].symbol()));
        }
    }
    shaded
}

/// Cells of the box still holding a glyph.
fn glyphs(buf: &Buffer) -> usize {
    let mut drawn = 0;
    for y in BOX.top()..BOX.bottom() {
        for x in BOX.left()..BOX.right() {
            drawn += usize::from(buf[(x, y)].symbol() != " ");
        }
    }
    drawn
}

/// Cells of the box's left half and right half that have been cleared away.
fn cleared(buf: &Buffer) -> (usize, usize) {
    let middle = BOX.left() + BOX.width / 2;
    let (mut left, mut right) = (0, 0);
    for y in BOX.top()..BOX.bottom() {
        for x in BOX.left()..BOX.right() {
            let gone = usize::from(buf[(x, y)].symbol() == " ");
            if x < middle {
                left += gone;
            } else {
                right += gone;
            }
        }
    }
    (left, right)
}

/// `SPEC.md` §11.2 B21: the box arrives through `tachyonfx` under §5.3's
/// licence, armed by the click and done when done, over what a receipt takes;
/// Esc sweeps it away over the same length.
///
/// The evolve needs no second ink, which is why the entrance is checked on the
/// symbols rather than on the colours: it is the one arrival on this surface
/// that a palette with no colour at all still shows.
#[test]
fn the_box_evolves_in_and_esc_sweeps_it_away() {
    let theme = Theme::default();
    let ring = 2 * (usize::from(BOX.width) + usize::from(BOX.height)) - 4;
    let inner = usize::from(BOX.width - 2) * usize::from(BOX.height - 2);

    // In: nothing of the box at the start, shade blocks part way through, and
    // the box exactly as the renderer drew it at the end.
    let mut entrance = box_entrance(&theme);
    let mut buf = boxed(&theme);
    entrance.process(FxDuration::ZERO, &mut buf, BOX);
    assert_eq!(
        drawn_of(&buf, &theme),
        (0, 0),
        "the box is drawn whole on the frame that opened it"
    );
    let mut spent = std::time::Duration::ZERO;
    let mut shaded = false;
    while spent + ARRIVING_FRAME < BOX_ARRIVING {
        let mut buf = boxed(&theme);
        entrance.process(FxDuration::from(ARRIVING_FRAME), &mut buf, BOX);
        spent += ARRIVING_FRAME;
        shaded |= shading(&buf) > 0;
        assert!(
            !entrance.done(),
            "the entrance reported itself done at {spent:?}, inside its {BOX_ARRIVING:?}"
        );
    }
    assert!(
        shaded,
        "the box never drew a shade block on its way in: it snapped"
    );
    let mut buf = boxed(&theme);
    entrance.process(
        FxDuration::from(BOX_ARRIVING - spent + ARRIVING_FRAME),
        &mut buf,
        BOX,
    );
    assert!(
        entrance.done(),
        "the entrance is still running past its length"
    );
    assert_eq!(
        drawn_of(&buf, &theme),
        (ring, inner),
        "the entrance did not end on the box as drawn"
    );

    // Out: swept away, and the sweep runs left to right, so the half it starts
    // on is the emptier one throughout.
    let mut exit = box_exit();
    let mut buf = boxed(&theme);
    exit.process(FxDuration::ZERO, &mut buf, BOX);
    assert_eq!(
        glyphs(&buf),
        ring + inner,
        "the box left before Esc was pressed"
    );
    let mut spent = std::time::Duration::ZERO;
    let mut lead = 0;
    while spent + ARRIVING_FRAME < BOX_ARRIVING {
        let mut buf = boxed(&theme);
        exit.process(FxDuration::from(ARRIVING_FRAME), &mut buf, BOX);
        spent += ARRIVING_FRAME;
        let (left, right) = cleared(&buf);
        lead = lead.max(left.saturating_sub(right));
        assert!(!exit.done(), "the exit reported itself done at {spent:?}");
    }
    assert!(
        lead > usize::from(BOX.height),
        "the sweep never cleared the box's left half ahead of its right, so it          is not travelling left to right: it led by {lead} cells at most"
    );
    let mut buf = boxed(&theme);
    exit.process(
        FxDuration::from(BOX_ARRIVING - spent + ARRIVING_FRAME),
        &mut buf,
        BOX,
    );
    assert!(exit.done(), "the exit is still running past its length");
    // On the glyphs rather than the inks: a sweep clears the cell's symbol and
    // leaves its colour, and the frame after this one drops the rows anyway.
    assert_eq!(glyphs(&buf), 0, "the exit did not end on the box gone");
}

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

#[test]
fn an_effect_armed_after_a_quiet_spell_starts_at_its_beginning() {
    // The loop paints on wakes alone, so the interval since the previous paint on
    // the first wake after a quiet spell is the whole spell, which an effect armed
    // on that wake has not lived through.
    let spell = std::time::Duration::from_secs(600);
    assert_eq!(
        effect_interval(false, spell),
        std::time::Duration::ZERO,
        "an effect armed after a quiet spell is told the spell passed"
    );
    assert_eq!(
        effect_interval(true, ARRIVING_FRAME),
        ARRIVING_FRAME,
        "an effect that was drawing is not told the frame that passed"
    );
    // And what the rule prevents, on the effects themselves: told the spell,
    // each half of a departure ends inside its first frame and the reader sees
    // none of it.
    for (name, mut effect) in [
        (
            "the agent's line arriving",
            resolve_arrival(&Theme::default()),
        ),
        ("the sweep that takes the rows", leaving()),
    ] {
        let mut buf = drawn();
        effect.process(FxDuration::from(spell), &mut buf, PANE);
        assert!(
            effect.done(),
            "{name} survives an interval longer than itself, so the rule above \
             guards nothing"
        );
    }
}

#[test]
fn a_departure_ends_inside_its_own_length() {
    // `SPEC.md` §11.2 B21: every effect a departure runs is armed by a wake and
    // ends inside its own duration, which is the licence I1 grants every effect.
    // The beat between the two carries no effect at all, so the pane asks for no
    // frame while the agent's line is simply being read; `tests/notes.rs` holds
    // that half. What is left here is that neither effect outlives its slot: the
    // ledger arms the sweep at `RESOLVED_DEPARTURE - LEAVING` and drops the rows
    // at `RESOLVED_DEPARTURE`.
    assert_eq!(
        RESOLVED_DEPARTURE,
        RESOLVE_ARRIVING + RESOLVE_BEAT + LEAVING,
        "the departure's length is not the sum of its three parts, so the rows \
         are dropped before or after the effect over them ends"
    );
    for (name, mut effect, length) in [
        (
            "a resolve's line",
            resolve_arrival(&Theme::default()),
            RESOLVE_ARRIVING,
        ),
        ("a withdrawal", leaving(), LEAVING),
    ] {
        let mut spent = std::time::Duration::ZERO;
        while spent + ARRIVING_FRAME < length {
            let mut buf = drawn();
            effect.process(FxDuration::from(ARRIVING_FRAME), &mut buf, PANE);
            spent += ARRIVING_FRAME;
            assert!(
                !effect.done(),
                "{name} reported itself done at {spent:?}, inside its {length:?}, \
                 so the rows are blank before the ledger reaches them"
            );
        }
        let mut buf = drawn();
        effect.process(
            FxDuration::from(length - spent + ARRIVING_FRAME),
            &mut buf,
            PANE,
        );
        assert!(
            effect.done(),
            "{name} is still running a frame past its {length:?}, so it would \
             hold the clock through the beat, or after the rows are gone"
        );
    }
}

#[test]
fn a_departure_changes_the_cells_it_covers_and_leaves_them_blank() {
    // The same gate the coalesce has: an effect that runs and leaves the buffer
    // as it found it is not visible from the arithmetic. And a dissolve has to
    // end on nothing, because the frame after it drops the rows and a glyph the
    // dissolve left would be seen leaving twice.
    let settled = drawn();
    let mut arrival = resolve_arrival(&Theme::default());
    let mut buf = drawn();
    arrival.process(FxDuration::from(ARRIVING_FRAME), &mut buf, PANE);
    assert_ne!(
        buf, settled,
        "one frame into a resolve's departure every cell is as the renderer left \
         it, so the agent's line arrives without arriving"
    );
    // Run out, the line stands where the renderer drew it, and it is what the
    // reader looks at for the whole beat with no effect over it.
    buf = drawn();
    arrival.process(FxDuration::from(RESOLVE_ARRIVING), &mut buf, PANE);
    assert_eq!(
        symbols(&buf),
        symbols(&settled),
        "the arrival ended on a glyph of its own, so the agent's line is not \
         readable while it holds"
    );

    // Then the sweep, which has to end on nothing. Halfway rather than one frame
    // in: its edge is soft over `SWEEP` columns and eased at both ends, so the
    // first frame has not reached the first cell yet.
    let mut sweep = leaving();
    buf = drawn();
    sweep.process(FxDuration::from(LEAVING / 2), &mut buf, PANE);
    assert_ne!(
        buf, settled,
        "halfway through the sweep every cell is as the renderer left it, so the \
         rows leave without leaving"
    );
    buf = drawn();
    sweep.process(
        FxDuration::from(LEAVING / 2 + ARRIVING_FRAME),
        &mut buf,
        PANE,
    );
    assert!(
        symbols(&buf).iter().all(|cell| cell == " "),
        "a dissolve run for its whole length left glyphs behind, which the frame \
         after it would drop twice"
    );
}
