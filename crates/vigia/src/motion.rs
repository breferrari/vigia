//! How this pane moves: its motions, written in `tachyonfx`'s own DSL, and the
//! two rules the loop owes every effect it arms.
//!
//! A motion is a source string and the function that binds its names. The crate
//! composes and times what it builds: a sequence reports its parts added up and
//! a parallel the longer of them, which is what [`Timed`] retires on, so nothing
//! here counts a duration the effect could be asked for.
//!
//! The durations are here for the reason the same table always is: copies
//! drift. `SPEC.md` §5.1 and §11.1 rule what each one is.

use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use tachyonfx::dsl::EffectDsl;
use tachyonfx::pattern::AnyPattern;
use tachyonfx::{Effect, fx};

/// How long a change is drawn arriving. Under `HISTORY_SAMPLE`; see `SPEC.md` §5.1.
pub const ARRIVING: Duration = Duration::from_millis(250);

/// A receipt's arrival.
pub const SAID_ARRIVING: Duration = Duration::from_millis(550);

/// An announcement's, its own rather than [`ARRIVING`], which is the diff's.
pub const NOTICE_ARRIVING: Duration = Duration::from_millis(750);

/// How long a warning takes to gather.
pub const ALERT_ARRIVING: Duration = Duration::from_millis(450);

/// How often a running effect asks for a frame. The whole price of the effect.
pub const ARRIVING_FRAME: Duration = Duration::from_millis(16);

/// The whole of a receipt's or a warning's time on the footer, both transitions
/// included. One-shot, so an idle pane owns no timer.
///
/// Long enough that the two ends are a real part of it rather than something to
/// get through: at the slowest voice, 750ms in, three seconds settled, 750 out.
pub const NOTICE_LINGER: Duration = Duration::from_millis(4500);

/// The whole of an announcement's time on the footer. A receipt answers a gesture
/// the reader just made and finds them looking; an announcement arrives while
/// they are looking at the other pane, and [`NOTICE_LINGER`] was gone before it
/// was read.
pub const ARRIVED_LINGER: Duration = Duration::from_secs(60);

/// How long the box takes to arrive, and to leave on Esc: what a changed file
/// takes.
pub const BOX_ARRIVING: Duration = ARRIVING;

/// How long a note's rows and the agent's line take to arrive: an
/// announcement's own arrival, since that is what they are.
pub const RESOLVE_ARRIVING: Duration = NOTICE_ARRIVING;

/// How long a note's rows take to leave, whichever way the note goes.
pub const LEAVING: Duration = NOTICE_ARRIVING;

/// The whole of a resolve's departure, after which the rows are dropped: one
/// notice's time on the footer, its two ends included. One table with the
/// footer's, so the pane keeps one rhythm.
pub const RESOLVED_DEPARTURE: Duration = NOTICE_LINGER;

/// How long a resolve's line holds between arriving and leaving.
pub const RESOLVE_BEAT: Duration = RESOLVED_DEPARTURE
    .saturating_sub(RESOLVE_ARRIVING)
    .saturating_sub(LEAVING);

/// A surface arriving: shade blocks resolving into what the renderer drew,
/// thickening out of the middle, with the cells landing in their own order
/// under them.
///
/// The pattern is named on each part and not on the pair. `Shader::set_pattern`
/// defaults to doing nothing and the containers do not override it, so a
/// pattern on a `parallel` is dropped in silence and every cell arrives at once.
///
/// Shade blocks rather than a crossfade because they are glyphs, so this is the
/// one arrival on the note surface that still draws where the depth has
/// flattened the palette and there are no two inks to travel between.
const EVOLVING: &str = r#"
    fx::parallel(&[
        fx::evolve_into((EvolveSymbolSet::Shaded, ink), (over, Linear))
            .with_pattern(RadialPattern::with_transition((0.5, 0.5), softness)),
        fx::coalesce((over, Linear)),
    ])
"#;

/// A surface leaving: swept away, left to right.
const SWEEPING: &str = r#"
    fx::dissolve((over, Linear)).with_pattern(SweepPattern::left_to_right(span))
"#;

/// A departure that shows something first: it arrives, holds a beat, and goes.
const HOLDING: &str = r#"
    fx::sequence(&[arriving, fx::sleep((beat, Linear)), leaving])
"#;

/// A message's ink arriving from the colour it replaces, by the road its voice
/// travels.
const FADING_IN: &str = r#"
    fx::fade_from_fg(ink, (over, SineInOut)).with_pattern(road)
"#;

/// The same ink leaving for that colour, by the road it came.
///
/// Not [`FADING_IN`] reversed: `fade_from_fg` mirrors its timer, so reversing it
/// flips the interpolation as well as the direction, and a voice would leave on
/// a curve it did not arrive on.
const FADING_OUT: &str = r#"
    fx::fade_to_fg(ink, (over, SineInOut)).with_pattern(road)
"#;

/// A change arriving on the diff: the cells landing in their own order.
const COALESCING: &str = r#"
    fx::coalesce((over, QuadOut))
"#;

/// The compilers a source is read against.
///
/// Built per motion rather than kept: `EffectDsl` holds its compilers as boxed
/// closures and is neither `Send` nor `Sync`, so a `static` cannot hold one, and
/// registering them is 6.3us against the 12.7us the compile itself costs. A
/// motion is compiled when it is armed, which is a press or a wake, never a
/// frame.
fn dsl() -> EffectDsl {
    EffectDsl::new()
}

/// A surface arriving, in `ink`, its edge soft over `softness` cells.
#[must_use]
pub fn evolving(ink: Style, over: Duration, softness: f32) -> Effect {
    compiled(
        dsl()
            .compiler()
            .bind("ink", ink)
            .bind("over", tachyonfx::Duration::from(over))
            .bind("softness", softness)
            .compile(EVOLVING),
    )
}

/// A surface leaving, its edge soft over `span` columns.
#[must_use]
pub fn sweeping(over: Duration, span: u32) -> Effect {
    compiled(
        dsl()
            .compiler()
            .bind("over", tachyonfx::Duration::from(over))
            .bind("span", span)
            .compile(SWEEPING),
    )
}

/// `arriving`, held for `beat`, then `leaving`.
#[must_use]
pub fn holding(arriving: Effect, beat: Duration, leaving: Effect) -> Effect {
    compiled(
        dsl()
            .compiler()
            .bind("arriving", arriving)
            .bind("beat", tachyonfx::Duration::from(beat))
            .bind("leaving", leaving)
            .compile(HOLDING),
    )
}

/// A message's ink arriving from `ink` by `road`, or leaving for it.
#[must_use]
pub fn fading(ink: Color, over: Duration, road: AnyPattern, out: bool) -> Effect {
    compiled(
        dsl()
            .compiler()
            .bind("ink", ink)
            .bind("over", tachyonfx::Duration::from(over))
            .bind("road", road)
            .compile(if out { FADING_OUT } else { FADING_IN }),
    )
}

/// A change arriving on the diff.
#[must_use]
pub fn coalescing(over: Duration) -> Effect {
    compiled(
        dsl()
            .compiler()
            .bind("over", tachyonfx::Duration::from(over))
            .compile(COALESCING),
    )
}

/// What a source compiles to, and what a source that does not compile draws.
///
/// Nothing, and the surface under it stands: this workspace aborts on a panic,
/// so a dead monitor is the alternative. The sources are this repository's own
/// and the suite compiles every one, so the second arm means a binary shipped
/// with a motion nobody can see rather than a reader's mistake.
fn compiled(built: Result<Effect, tachyonfx::dsl::DslParseError>) -> Effect {
    built.unwrap_or_else(|_| fx::sleep(tachyonfx::Duration::from(Duration::ZERO)))
}

/// An effect and the moment it is retired at.
pub struct Timed {
    effect: Effect,
    /// The clock is the retirement and the effect's own count is the other
    /// half: a thing off screen is never processed, and an effect never
    /// processed never reports itself done.
    until: Instant,
}

impl Timed {
    /// Arm `effect` for as long as it says it runs, which for a composed one is
    /// its parts added up, or the longest of them, by the crate's own count.
    #[must_use]
    pub fn armed(effect: Effect, now: Instant) -> Self {
        Self {
            until: now + length(&effect),
            effect,
        }
    }

    /// Whether it still has frames to draw, which keeps the frame clock armed.
    #[must_use]
    pub fn is_running(&self) -> bool {
        !self.effect.done()
    }

    /// Whether it has run its length, by its own count or by the clock.
    #[must_use]
    pub fn spent(&self, now: Instant) -> bool {
        now >= self.until || self.effect.done()
    }

    /// Advance it by `since` over `over`, the cells its subject drew this frame.
    pub fn draw(&mut self, since: Duration, buf: &mut Buffer, over: Rect) {
        self.effect.process(since.into(), buf, over);
    }
}

/// How long `effect` runs, as it reports itself; nothing at all for one that
/// carries no timer, which retires it on the frame that armed it.
#[must_use]
pub fn length(effect: &Effect) -> Duration {
    effect
        .timer()
        .map_or(Duration::ZERO, |timer| timer.duration().into())
}

/// The time an effect is told passed since the previous paint, given whether an
/// effect was drawing then. The loop paints on wakes alone, so `since_paint` on
/// the first wake after a quiet spell is the whole of the spell; an effect armed
/// on that wake has lived through none of it, and told all of it a departure
/// would end inside its first frame. While one was drawing, the loop was
/// painting at its cadence and the interval is the frame it says.
#[must_use]
pub fn effect_interval(ran: bool, since_paint: Duration) -> Duration {
    if ran { since_paint } else { Duration::ZERO }
}

#[cfg(test)]
mod tests {
    //! The arm no drawn screen reaches: what a source that will not compile
    //! leaves behind. Everything else here is gated on the pane in
    //! `tests/motion.rs`, which compiles every source this binary ships.

    use super::*;

    #[test]
    fn a_source_that_will_not_compile_draws_nothing_and_retires_at_once() {
        let broken = dsl().compiler().compile("fx::a_motion_nobody_wrote(");
        assert!(broken.is_err(), "the DSL accepted a source that is not one");

        let effect = compiled(broken);
        assert_eq!(
            length(&effect),
            Duration::ZERO,
            "a source that will not compile armed an effect with a length, so              the cells under it are held for a motion that never draws"
        );

        // And it changes nothing, so the surface the renderer drew stands.
        let over = Rect::new(0, 0, 8, 2);
        let settled = ratatui::buffer::Buffer::empty(over);
        let mut buf = settled.clone();
        let mut effect = effect;
        effect.process(tachyonfx::Duration::from(ARRIVING), &mut buf, over);
        assert_eq!(buf, settled);
        assert!(effect.done());
    }
}
