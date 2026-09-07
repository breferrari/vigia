//! How this pane moves: the motions a surface can arm, composed rather than
//! hand-built, and the two rules every armed effect here obeys.
//!
//! A [`Motion`] carries its **length** as well as its effect, and that is the
//! whole reason this is a type rather than a handful of constructors.
//! [`Timed`] retires an effect by a clock as well as by the effect's own count,
//! because an effect over cells that are off screen is never processed and
//! never reports itself done; so every call site needs the duration it armed,
//! and a composed one needs the sum. Composing the effect and adding the
//! durations up by hand are two chances to disagree.
//!
//! The durations are here for the reason the same table always is: copies
//! drift. `SPEC.md` §5.1 and §11.1 rule what each one is.

use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use tachyonfx::fx::EvolveSymbolSet;
use tachyonfx::pattern::{RadialPattern, SweepPattern};
use tachyonfx::{Effect, Interpolation, fx};

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

/// Columns the colour's leading edge is soft over as it crosses a message.
pub const TRAVEL: u16 = 12;

/// The same, for the warning that resolves from both ends at once.
pub const TRAVEL_IN: f32 = 10.0;

/// The radial edge's softness where a note's cells arrive, in cells.
pub const TRANSITION: f32 = 10.0;

/// Columns the sweep's leading edge is soft over as it clears a note away.
pub const SWEEP: u16 = 35;

/// What a motion does to the cells it runs over.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    /// Shade blocks resolving into whatever the cells already hold.
    Evolve,
    /// Cells clearing away.
    Sweep,
    /// Cells landing, each in its own moment.
    Coalesce,
    /// The ink travelling from a colour into the one the cells are drawn in.
    CrossfadeIn(Color),
    /// The same journey back out.
    CrossfadeOut(Color),
    /// Nothing at all, for the beat between two motions.
    Still,
}

/// Where a motion's edge is, and which way it travels.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Spread {
    /// Every cell at once.
    Whole,
    /// Out of the middle, soft over this many cells.
    Centre(f32),
    /// Across, soft over this many columns.
    LeftToRight(u16),
    /// The same, the other way.
    RightToLeft(u16),
}

/// One motion, before it is composed with any other.
#[derive(Debug, Clone, Copy)]
struct One {
    kind: Kind,
    ink: Option<Style>,
    spread: Spread,
    ease: Interpolation,
    over: Duration,
}

/// One motion, or several that run in order or at once.
#[derive(Debug, Clone)]
enum Shape {
    One(One),
    /// One after another.
    Order(Vec<Motion>),
    /// All at the same time.
    Together(Vec<Motion>),
}

/// A motion this pane can arm: what it does, how its edge spreads, what ink it
/// does it in, and how long the whole of it takes.
///
/// Built by naming the motion, then the way it moves, then anything it runs
/// with or after:
///
/// ```ignore
/// Motion::evolve(RESOLVE_ARRIVING)
///     .ink(theme.note_reply)
///     .from_centre(TRANSITION)
///     .hold(RESOLVE_BEAT)
///     .then(Motion::sweep(LEAVING).across(SWEEP))
/// ```
#[derive(Debug, Clone)]
pub struct Motion {
    shape: Shape,
    length: Duration,
}

impl Motion {
    /// Shade blocks resolving into the text, which is how this pane says a
    /// surface has arrived. Unlike a crossfade it needs no second ink, so it
    /// draws on a palette that has no colour at all.
    #[must_use]
    pub fn evolve(over: Duration) -> Self {
        Self::one(Kind::Evolve, over)
    }

    /// Cells clearing away, which is how it says one has gone.
    #[must_use]
    pub fn sweep(over: Duration) -> Self {
        Self::one(Kind::Sweep, over)
    }

    /// Cells landing, each in its own moment.
    #[must_use]
    pub fn coalesce(over: Duration) -> Self {
        Self::one(Kind::Coalesce, over)
    }

    /// The ink arriving from `from` into whatever the cells are drawn in.
    #[must_use]
    pub fn crossfade_in(from: Color, over: Duration) -> Self {
        Self::one(Kind::CrossfadeIn(from), over)
    }

    /// The ink leaving for `to`, which is the journey above run the other way.
    ///
    /// Not the arrival reversed: `fade_from_fg` mirrors its timer, so reversing
    /// it flips the interpolation as well as the direction, and a voice would
    /// leave on a different curve than it arrived on.
    #[must_use]
    pub fn crossfade_out(to: Color, over: Duration) -> Self {
        Self::one(Kind::CrossfadeOut(to), over)
    }

    /// Nothing happening, for the beat a departure holds before it leaves.
    #[must_use]
    pub fn still(over: Duration) -> Self {
        Self::one(Kind::Still, over)
    }

    /// The ink an evolve's shade blocks take. Every other motion reads the ink
    /// the cells already carry.
    #[must_use]
    pub fn ink(mut self, style: Style) -> Self {
        self.each(&|one| one.ink = Some(style));
        self
    }

    /// Out of the middle, its edge soft over `softness` cells.
    #[must_use]
    pub fn from_centre(mut self, softness: f32) -> Self {
        self.each(&|one| one.spread = Spread::Centre(softness));
        self
    }

    /// Across, its edge soft over `span` columns.
    #[must_use]
    pub fn across(mut self, span: u16) -> Self {
        self.each(&|one| one.spread = Spread::LeftToRight(span));
        self
    }

    /// The same, the other way.
    #[must_use]
    pub fn back(mut self, span: u16) -> Self {
        self.each(&|one| one.spread = Spread::RightToLeft(span));
        self
    }

    /// The curve it runs on.
    #[must_use]
    pub fn eased(mut self, how: Interpolation) -> Self {
        self.each(&|one| one.ease = how);
        self
    }

    /// This, and then `next`. The length is the two added, which is what the
    /// clock retiring them both needs.
    #[must_use]
    pub fn then(self, next: Self) -> Self {
        let length = self.length.saturating_add(next.length);
        let parts = match self.shape {
            Shape::Order(mut parts) => {
                parts.push(next);
                parts
            }
            shape => vec![
                Self {
                    shape,
                    length: self.length,
                },
                next,
            ],
        };
        Self {
            shape: Shape::Order(parts),
            length,
        }
    }

    /// This, then a pause of `beat` before whatever follows.
    #[must_use]
    pub fn hold(self, beat: Duration) -> Self {
        self.then(Self::still(beat))
    }

    /// This and `other` over the same cells at once, the second drawn over the
    /// first. The length is the longer of the two.
    #[must_use]
    pub fn with(self, other: Self) -> Self {
        let length = self.length.max(other.length);
        let parts = match self.shape {
            Shape::Together(mut parts) => {
                parts.push(other);
                parts
            }
            shape => vec![
                Self {
                    shape,
                    length: self.length,
                },
                other,
            ],
        };
        Self {
            shape: Shape::Together(parts),
            length,
        }
    }

    /// How long the whole of it takes.
    #[must_use]
    pub fn length(&self) -> Duration {
        self.length
    }

    /// The effect it builds.
    #[must_use]
    pub fn effect(self) -> Effect {
        match self.shape {
            Shape::One(one) => one.effect(),
            Shape::Order(parts) => fx::sequence(&Self::built(parts)),
            Shape::Together(parts) => fx::parallel(&Self::built(parts)),
        }
    }

    /// The effect, armed until its own length is up.
    #[must_use]
    pub fn armed(self, now: Instant) -> Timed {
        let until = now + self.length;
        Timed::new(self.effect(), until)
    }

    fn one(kind: Kind, over: Duration) -> Self {
        Self {
            shape: Shape::One(One {
                kind,
                ink: None,
                spread: Spread::Whole,
                ease: Interpolation::Linear,
                over,
            }),
            length: over,
        }
    }

    /// Apply `f` to every motion inside this one, so a spread or an ink named
    /// after two are composed reaches both.
    fn each(&mut self, f: &dyn Fn(&mut One)) {
        match &mut self.shape {
            Shape::One(one) => f(one),
            Shape::Order(parts) | Shape::Together(parts) => {
                for part in parts {
                    part.each(f);
                }
            }
        }
    }

    fn built(parts: Vec<Self>) -> Vec<Effect> {
        parts.into_iter().map(Self::effect).collect()
    }
}

impl One {
    fn effect(self) -> Effect {
        let timer = (tachyonfx::Duration::from(self.over), self.ease);
        let effect = match self.kind {
            // Into rather than plain: it stops overwriting at the end, so the
            // cells' own content is what is left when it is done.
            Kind::Evolve => match self.ink {
                Some(style) => fx::evolve_into((EvolveSymbolSet::Shaded, style), timer),
                None => fx::evolve_into(EvolveSymbolSet::Shaded, timer),
            },
            Kind::Sweep => fx::dissolve(timer),
            Kind::Coalesce => fx::coalesce(timer),
            Kind::CrossfadeIn(from) => fx::fade_from_fg(from, timer),
            Kind::CrossfadeOut(to) => fx::fade_to_fg(to, timer),
            Kind::Still => fx::sleep(timer),
        };
        match self.spread {
            Spread::Whole => effect,
            Spread::Centre(softness) => {
                effect.with_pattern(RadialPattern::with_transition((0.5, 0.5), softness))
            }
            Spread::LeftToRight(span) => effect.with_pattern(SweepPattern::left_to_right(span)),
            Spread::RightToLeft(span) => effect.with_pattern(SweepPattern::right_to_left(span)),
        }
    }
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
    /// Arm `effect` until `until`.
    #[must_use]
    pub fn new(effect: Effect, until: Instant) -> Self {
        Self { effect, until }
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
