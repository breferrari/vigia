//! What the footer says a message is: its colour, and the way it arrives and
//! leaves.
//!
//! Every assertion here is about drawn cells rather than about arithmetic, which
//! is `arriving.rs`'s lesson one region over: an effect that runs and leaves the
//! buffer as it found it passes every test written about its numbers.

use std::time::Instant;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use tachyonfx::{Duration as FxDuration, EffectManager, Interpolation, Motion, fx};
use vigia::{
    ALERT_ARRIVING, ARRIVING, ARRIVING_FRAME, App, Glyphs, NOTICE_LINGER, Pointing, SAID_ARRIVING,
    Theme, View, Voice, notice_area, render,
};

/// An ordinary pane.
const PANE: Rect = Rect {
    x: 0,
    y: 0,
    width: 80,
    height: 12,
};

/// The three voices, so a case added to one is added to all.
const VOICES: [Voice; 3] = [Voice::Said, Voice::Arrived, Voice::Alert];

/// A message of `voice` on the footer, and the pane it draws.
fn drawn(voice: Voice) -> (Buffer, App) {
    let mut app = App::new();
    app.flash(
        "sent 3 lines to the clipboard",
        Instant::now() + NOTICE_LINGER,
        voice,
    );
    let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
    let mut buf = Buffer::empty(PANE);
    render(
        &mut buf,
        PANE,
        &View::default(),
        &Theme::default(),
        Glyphs::default(),
        &chrome,
    );
    (buf, app)
}

/// The cells of a row, as strings, so a comparison names what changed.
fn symbols(buf: &Buffer, area: Rect) -> Vec<String> {
    (area.y..area.y + area.height)
        .flat_map(|y| (area.x..area.x + area.width).map(move |x| (x, y)))
        .map(|(x, y)| buf[(x, y)].symbol().to_owned())
        .collect()
}

/// The styles of a row, which is what a colour change moves and a glyph one does not.
fn styles(buf: &Buffer, area: Rect) -> Vec<Style> {
    (area.x..area.x + area.width)
        .map(|x| buf[(x, area.y)].style())
        .collect()
}

/// The area the footer's message occupies on this pane.
fn area_of(app: &App) -> Rect {
    let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
    notice_area(PANE, &chrome).expect("a message on the footer has an area")
}

#[test]
fn each_voice_takes_its_own_colour() {
    let mut seen: Vec<Vec<Style>> = Vec::new();
    for voice in VOICES {
        let (buf, app) = drawn(voice);
        seen.push(styles(&buf, area_of(&app)));
    }
    for (at, one) in seen.iter().enumerate() {
        for other in seen.iter().skip(at + 1) {
            assert_ne!(
                one, other,
                "two voices drew the message in the same style, so the footer \
                 says the same thing about a receipt and a failure"
            );
        }
    }
}

#[test]
fn a_message_is_not_drawn_in_the_hints_own_style() {
    // The floor this was reported against: everything on the footer was one
    // colour, so nothing on it meant anything.
    let mut app = App::new();
    let hints = {
        let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
        let mut buf = Buffer::empty(PANE);
        render(
            &mut buf,
            PANE,
            &View::default(),
            &Theme::default(),
            Glyphs::default(),
            &chrome,
        );
        buf[(2, PANE.height - 1)].style()
    };
    app.flash(
        "sent 3 lines to the clipboard",
        Instant::now() + NOTICE_LINGER,
        Voice::Said,
    );
    let (buf, app) = (
        {
            let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
            let mut buf = Buffer::empty(PANE);
            render(
                &mut buf,
                PANE,
                &View::default(),
                &Theme::default(),
                Glyphs::default(),
                &chrome,
            );
            buf
        },
        app,
    );
    let area = area_of(&app);
    assert_ne!(
        buf[(area.x, area.y)].style(),
        hints,
        "a message reads exactly like the hints it replaced"
    );
}

/// The gate `arriving.rs` exists for, one region over, and per voice because
/// three effects are three separate ways to ship nothing.
#[test]
fn every_voice_changes_the_cells_it_covers_as_it_arrives() {
    for voice in VOICES {
        let (settled, app) = drawn(voice);
        let area = area_of(&app);
        let mut buf = settled.clone();
        let mut effects: EffectManager<String> = EffectManager::default();
        effects.add_unique_effect("notice".to_owned(), arrival_of(voice));
        effects.process_effects(FxDuration::from(ARRIVING_FRAME), &mut buf, area);
        assert_ne!(
            (symbols(&buf, area), styles(&buf, area)),
            (symbols(&settled, area), styles(&settled, area)),
            "{voice:?} ran and left every cell as it found it, so nothing reaches the pane"
        );
    }
}

#[test]
fn every_voice_arrives_differently() {
    let mut seen = Vec::new();
    for voice in VOICES {
        let (settled, app) = drawn(voice);
        let area = area_of(&app);
        let mut buf = settled.clone();
        let mut effects: EffectManager<String> = EffectManager::default();
        effects.add_unique_effect("notice".to_owned(), arrival_of(voice));
        effects.process_effects(FxDuration::from(ARRIVING_FRAME), &mut buf, area);
        seen.push((symbols(&buf, area), styles(&buf, area)));
    }
    for (at, one) in seen.iter().enumerate() {
        for other in seen.iter().skip(at + 1) {
            assert_ne!(
                one, other,
                "two voices arrive identically, so the table that maps them to \
                 effects has two rows saying one thing"
            );
        }
    }
}

#[test]
fn a_settled_arrival_is_what_the_renderer_drew() {
    // It has to end where the ordinary render ends, or the line keeps a shape
    // the renderer never gives it.
    for voice in VOICES {
        let (settled, app) = drawn(voice);
        let area = area_of(&app);
        let mut buf = settled.clone();
        let mut effects: EffectManager<String> = EffectManager::default();
        effects.add_unique_effect("notice".to_owned(), arrival_of(voice));
        effects.process_effects(FxDuration::from(ARRIVING * 4), &mut buf, area);
        assert_eq!(
            (symbols(&buf, area), styles(&buf, area)),
            (symbols(&settled, area), styles(&settled, area)),
            "{voice:?} finished somewhere other than the drawn line. A voice whose              arrival is motionless settles only in style, so comparing glyphs              alone cannot see it land on the wrong colour"
        );
    }
}

#[test]
fn the_area_is_the_bottom_row_and_only_the_message() {
    let (_, app) = drawn(Voice::Said);
    let area = area_of(&app);
    assert_eq!(area.height, 1);
    assert_eq!(
        area.y,
        PANE.height - 1,
        "the message is not on the pane's bottom row, which is where it is drawn"
    );
    assert!(
        area.width <= PANE.width && area.width >= 20,
        "the area is {} columns for a 29-character message",
        area.width
    );
    assert!(area.x > 0, "the area ignores the pane's own inset");
}

#[test]
fn a_pane_with_no_message_has_no_area() {
    let app = App::new();
    let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
    assert_eq!(notice_area(PANE, &chrome), None);
}

#[test]
fn a_pane_with_no_room_has_no_area() {
    let (_, app) = drawn(Voice::Said);
    let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
    for empty in [
        Rect::new(0, 0, 0, 12),
        Rect::new(0, 0, 80, 0),
        Rect::new(0, 0, 0, 0),
    ] {
        assert_eq!(
            notice_area(empty, &chrome),
            None,
            "a {empty:?} pane was given an area to draw an effect over"
        );
    }
}

/// An effect that never reports itself finished pins the loop awake for the
/// rest of the session, because `Shell::patience` asks for a frame while one is
/// running. `ping_pong` was tried here and does exactly that: it repeats, so
/// `is_running` never goes false and an idle monitor wakes every 16ms forever.
///
/// I1's budget is zero wakeups on an idle pane, and nothing else here can see
/// this: the pane looks right, and the cost is a fan.
#[test]
fn every_effect_finishes_and_stops_asking_for_frames() {
    for voice in VOICES {
        for (what, effect) in [
            ("arrival", arrival_of(voice)),
            ("departure", departure_of(voice)),
        ] {
            let (settled, app) = drawn(voice);
            let area = area_of(&app);
            let mut buf = settled.clone();
            let mut effects: EffectManager<String> = EffectManager::default();
            effects.add_unique_effect("notice".to_owned(), effect);
            assert!(
                effects.is_running(),
                "{voice:?}'s {what} was over before it began"
            );
            effects.process_effects(FxDuration::from(ARRIVING * 4), &mut buf, area);
            assert!(
                !effects.is_running(),
                "{voice:?}'s {what} still wants frames after four times its longest                  duration, so an idle pane never stops waking"
            );
        }
    }
}

/// The departures, spelled here for `arrival_of`'s reason.
fn departure_of(voice: Voice) -> tachyonfx::Effect {
    let theme = Theme::default();
    let dur = match voice {
        Voice::Said => SAID_ARRIVING,
        Voice::Arrived => ARRIVING,
        Voice::Alert => ALERT_ARRIVING,
    };
    match voice {
        Voice::Said => fx::sweep_out(
            Motion::LeftToRight,
            8,
            0,
            theme.chrome_dim.fg.unwrap_or(ratatui::style::Color::Reset),
            (FxDuration::from(dur), Interpolation::QuadIn),
        ),
        Voice::Arrived => fx::dissolve((FxDuration::from(dur), Interpolation::QuadIn)),
        Voice::Alert => fx::fade_to_fg(
            theme.chrome_dim.fg.unwrap_or(ratatui::style::Color::Reset),
            (FxDuration::from(dur), Interpolation::QuadIn),
        ),
    }
}

/// The arrivals, spelled here rather than reached for, so this suite fails when
/// the shell's table changes rather than following it silently.
fn arrival_of(voice: Voice) -> tachyonfx::Effect {
    let theme = Theme::default();
    match voice {
        Voice::Said => fx::sweep_in(
            Motion::LeftToRight,
            8,
            0,
            theme.chrome_dim.fg.unwrap_or(ratatui::style::Color::Reset),
            (FxDuration::from(SAID_ARRIVING), Interpolation::QuadOut),
        ),
        Voice::Arrived => fx::coalesce((FxDuration::from(ARRIVING), Interpolation::QuadOut)),
        Voice::Alert => fx::fade_from_fg(
            theme.chrome_dim.fg.unwrap_or(ratatui::style::Color::Reset),
            (FxDuration::from(ALERT_ARRIVING), Interpolation::QuadOut),
        ),
    }
}
