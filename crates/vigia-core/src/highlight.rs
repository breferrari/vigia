//! Incremental re-highlighting: I2b.
//!
//! > Re-highlighting is incremental. Only changed hunks are re-parsed, and the
//! > cost follows the edit rather than the file.
//!
//! `syntect` parses a line at a time, and every line's answer depends on the
//! state the line before it left behind. That single fact decides the whole
//! shape of this module, because it means highlighting cannot be a pure function
//! of the line the way [`Worktree::diff`](crate::Worktree::diff) is a pure
//! function of one file's two sides.
//!
//! Measured on this repository's own budget fixture, release build:
//!
//! | | |
//! |---|---|
//! | Load the bundled grammars | 318µs, against I7's 50ms |
//! | Parse one screenful, 24 lines | 1.53ms, against I9's 16ms |
//! | Parse a 1006-line hunk **whole** | **60.97ms**, which is 3.8x over I9 |
//! | Hash that hunk to revalidate it | 7.1µs |
//!
//! The third row is the one the design is built around, and it is the same
//! finding I2a made about re-diffing: the obvious implementation breaks the
//! frame budget on its own rather than merely wasting work. Under I9's own
//! shape, one line rewritten before every frame, parsing a hunk whole would
//! re-parse a thousand lines every frame. So **a hunk is parsed forward only as
//! far as something has asked for**, and what it has parsed is kept.
//!
//! Three decisions follow, and each is a budget rather than a preference.
//!
//! **A hunk is identified by its content, never by a generation counter.** A
//! counter bumped whenever the frame path recomputes a diff would be free to
//! check and wrong: inside the two-second settle margin the frame path
//! legitimately re-diffs an untouched file on every frame, so every hunk of it
//! would look new and I2b would fail on files nobody edited. Hashing 44 KiB
//! costs 7.1µs, is exact where it matters, and its one failure mode is a
//! sixty-four-bit collision showing stale colour rather than stale content.
//!
//! **Two parse states per hunk, one for each side of the diff.** Context feeds
//! both, a removal feeds the index side and an addition feeds the working-tree
//! side. Running the display order through a single state instead is cheaper and
//! visibly wrong: removing a line that opens a string and adding one that opens
//! it again applies the construct twice, and the rest of the hunk turns into
//! string. The cost is that a context line is parsed twice, which is at most six
//! lines per change group and none at all in an all-additions hunk.
//!
//! **The cache is bounded by the viewport**, not by the diff and not by the
//! session. [`Highlighter::pass`] hands out a guard that drops everything the
//! frame did not draw, and it sweeps in `Drop` rather than asking a caller to,
//! so a bulk edit across ten thousand files cannot grow it: the screen is the
//! bound. That is a stronger claim than the frame path's own, which is bounded
//! by the current diff, and it is what keeps I3 out of reach of this module. It
//! costs a re-parse when a hunk scrolls off and back, and a screenful is 1.53ms.
//!
//! **A changed hunk rewinds rather than starting over.** Throwing the parse away
//! made a frame cost the reader's scroll depth rather than what it drew, on
//! every frame, for as long as the file being read was the file being written:
//! 53ms p99 five hundred rows in. See [`CHECKPOINT_STRIDE`].
//!
//! What this module does **not** do is decide a colour. It maps a syntax scope
//! onto one of nine [`Class`]es and stops, because `SPEC.md` §6 puts no terminal
//! in this crate and §11.1 leaves the palette to the shell.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::io::Read as _;
use std::path::Path;
use std::sync::Arc;

use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};

use crate::hunk::{Hunk, Line, LineKind};

/// What a run of characters means, with no colour attached.
///
/// Nine, and the five the README mockup names are load bearing rather than a
/// starting point: `assets/preview.svg` is published, so which distinctions are
/// worth a colour was decided before this code existed (`SPEC.md` §5.1). String,
/// number and comment are added because a diff of real source is unreadable
/// without them and the mockup's sample lines happen not to contain one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Anything with no meaning worth colouring, which is most of a line.
    Plain,
    /// `fn`, `if`, `pub`, `mut`.
    Keyword,
    /// A type's name, whether built in or declared.
    Type,
    /// A function's name, at its definition or its call.
    Function,
    /// A binding, a parameter, a field.
    Variable,
    /// A named constant, and a language literal like `true`.
    Constant,
    /// A string literal, quotes included.
    String,
    /// A numeric literal.
    Number,
    /// A comment of any shape, its delimiters included.
    Comment,
}

/// A run of bytes within one line, and what it means.
///
/// `len` is in **bytes of the line's own text**, so the spans of a line always
/// sum to `text.len()` exactly. That is what lets a renderer walk a line and its
/// classes together without a second pass to reconcile them, and it is asserted
/// rather than assumed: see the tests at the bottom of this file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// Bytes this run covers.
    pub len: usize,
    /// What those bytes mean.
    pub class: Class,
}

/// What a [`Highlighter`] has done since it was created.
///
/// Cumulative, exactly like [`FrameStats`](crate::FrameStats), so a test
/// describes one frame by subtracting two readings. I2b is a claim about that
/// subtraction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HighlightStats {
    /// Hunks parsed from scratch: never seen, or seen with different content.
    pub parsed: u64,
    /// Hunks whose parse survived from an earlier frame.
    pub reused: u64,
    /// Lines actually run through the parser.
    ///
    /// A reuse adds nothing here, and neither does a file whose type nothing
    /// recognises, because neither one parses anything.
    pub lines: u64,
    /// Bytes of those lines.
    ///
    /// The number I2b is written against: what a re-highlight costs has to
    /// follow the edit, not the size of the file it landed in.
    pub bytes: u64,
    /// Hunks dropped because they left the viewport.
    ///
    /// I3 forbids unbounded growth over days, and this is what says the cache is
    /// bounded by the screen rather than by everything ever drawn.
    pub evicted: u64,
}

/// Scope prefixes and what each one means, **most specific first**.
///
/// Order is the whole of the table's correctness, because the first prefix that
/// matches wins. `constant.numeric` has to precede `constant` or no literal is
/// ever a number, and `storage.type.function` has to precede `storage.type`
/// because Sublime's Rust grammar scopes `fn` as a storage *type* while a reader
/// and the mockup both see a keyword.
const CLASSES: [(&str, Class); 16] = [
    ("comment", Class::Comment),
    ("string", Class::String),
    ("constant.numeric", Class::Number),
    ("constant", Class::Constant),
    ("keyword", Class::Keyword),
    ("storage.modifier", Class::Keyword),
    ("storage.type.function", Class::Keyword),
    ("storage.type", Class::Type),
    ("storage", Class::Keyword),
    ("entity.name.function", Class::Function),
    ("entity.name", Class::Type),
    ("support.function", Class::Function),
    ("support.type", Class::Type),
    ("support.class", Class::Type),
    ("variable.language", Class::Keyword),
    ("variable", Class::Variable),
];

/// Lines between the parse positions a later frame can rewind to.
///
/// The constant that turns "re-parse the whole prefix" into "re-parse a bounded
/// tail of it", and it is load bearing rather than a tuning knob.
///
/// Without it a hunk whose content changed threw its whole parse away, so a
/// frame cost the reader's **scroll depth** rather than what it drew. That is
/// paid on every frame in exactly the situation the tool exists for: the file
/// being written is the file being read, its hunk changes before every frame,
/// and a reader who scrolled in to follow along never gets a stable hunk to
/// amortise against. Measured at five hundred rows deep, before this existed:
/// 29ms p50 and 53ms p99 against a 16ms budget, sustained, with no input.
///
/// The trade is memory against that tail. A rewind re-parses at most one stride
/// plus what is drawn, and each stride costs one cloned parse position for as
/// long as the reader stays that deep in the hunk. Thirty-two keeps the tail
/// near two screenfuls while a thousand-line hunk holds thirty-one positions.
pub const CHECKPOINT_STRIDE: usize = 32;

/// Hunks kept after they have left the screen.
///
/// [`CHECKPOINT_STRIDE`] bounds re-parsing a hunk the reader is *in*. This bounds
/// re-parsing one they have come **back** to, which is the other half of the same
/// cost and the one nothing covered until
/// [#45](https://github.com/breferrari/vigia/issues/45).
///
/// The asymmetry is what makes it necessary. Scrolling down enters a hunk at its
/// top, where a forward-only parse has nothing above it to pay for; scrolling up
/// enters the same hunk at its **bottom** and pays the whole walk. Measured over
/// 120-row hunks of Japanese and emoji, release: a frame entering one from below
/// is **26.39ms** against the 16ms I9 budget, while the same frame with the parse
/// still held is **90µs**. So a reader flicking the wheel back over what they
/// just read dropped a frame per file, for an answer that had been in memory one
/// frame earlier.
///
/// **Four, because the bound has to stay a bound.** This is a constant added to
/// "what one screen can show", not a second cache that grows: a monitor is left
/// open for days, and I3 is the invariant that would pay for getting this wrong.
/// Four covers a wheel flick and its reversal, which is a screen or two of travel
/// and one or two hunks each way, with room for the hunk half off each edge.
/// Beyond that the parse is re-paid, and that is deliberate rather than
/// unnoticed.
///
/// **It is a count, and a count does not bound memory.** An entry holds one
/// `Vec<Span>` per line it has parsed and one checkpoint per stride, so four
/// thousand-line hunks are a few hundred kilobytes while four screen-sized ones
/// are a few tens. That is a *higher plateau* rather than growth, which is what
/// lets I3 live with it: drift compares a window against itself and cannot see a
/// level. It is also not a new shape, because a live entry already costs its
/// deepest parse rather than its screenful, so a retained one is one more of the
/// same rather than something worse. A bound in lines retained is the
/// byte-honest form and it costs the `tracked() <= WINDOW + RETAINED` gates their
/// exact form, which is a trade rather than an improvement.
///
/// **Why a queue and not a generation counter on the entry.** The counter looks
/// cheaper, and `History` next door keys recency exactly that way. It was tried
/// here and is worse for one reason: eviction needs a *total order over stale
/// entries*, and insertion order gives the queue one for free. A counter has to
/// rebuild it, because every hunk that leaves on the same frame shares a
/// generation, so keeping "the four newest" becomes a rank selection plus a
/// tie-break rule, and retaining by generation instead silently changes the
/// bound from `live + 4` to `live + everything drawn in the last four
/// generations`. More code, and a weaker bound.
pub const RETAINED_HUNKS: usize = 4;

/// Both sides of one hunk, each parsed as the file it describes.
///
/// `Clone` because a [`Checkpoint`] is one of these frozen at a line boundary.
/// The newline scratch buffer is deliberately **not** in here: it lives on
/// [`Entry`], so that cloning a parse position does not clone a buffer whose
/// contents are worthless one line later.
#[derive(Clone)]
struct Sides {
    /// The index side: context and removals.
    old: Side,
    /// The working-tree side: context and additions.
    new: Side,
}

/// One file's parse position: where the grammar is, and what scope it is under.
///
/// The two travel together always, and pairing them structurally is not tidiness:
/// with four flat fields, `spans_of(&mut self.old, &mut self.new_stack, ..)`
/// compiles and colours a removal against the addition that replaced it.
#[derive(Clone)]
struct Side {
    state: ParseState,
    stack: ScopeStack,
}

impl Side {
    fn new(syntax: &SyntaxReference) -> Self {
        Self {
            state: ParseState::new(syntax),
            stack: ScopeStack::new(),
        }
    }

    /// Run a line through this side without building spans for it.
    ///
    /// What a context line costs the side it is not drawn from.
    fn advance(&mut self, buf: &str, syntaxes: &SyntaxSet) {
        let Ok(ops) = self.state.parse_line(buf, syntaxes) else {
            return;
        };
        for (_, op) in &ops {
            if self.stack.apply(op).is_err() {
                return;
            }
        }
    }

    /// Run a line through this side and turn its scope changes into spans.
    fn spans(
        &mut self,
        buf: &str,
        line: &Line,
        syntaxes: &SyntaxSet,
        table: &[(Scope, Class)],
    ) -> Vec<Span> {
        let text_len = line.text.len();
        // A grammar that fails on a line leaves that line uncoloured rather than
        // failing the frame. A monitor survives the file it cannot read;
        // `SPEC.md` §2 makes a runtime measured in days turn every transient
        // failure into a certainty.
        let Ok(ops) = self.state.parse_line(buf, syntaxes) else {
            return plain(text_len);
        };

        let mut spans: Vec<Span> = Vec::new();
        let mut at = 0usize;
        for (offset, op) in &ops {
            // Clamped, because `buf` carries the newline the grammars need and
            // the line does not. Everything at or past the end contributes no
            // span while still advancing the state the next line starts from.
            let offset = (*offset).min(text_len);
            if offset > at {
                push(&mut spans, offset - at, classify(&self.stack, table));
                at = offset;
            }
            if self.stack.apply(op).is_err() {
                break;
            }
        }
        push(&mut spans, text_len - at, classify(&self.stack, table));
        spans
    }
}

impl Sides {
    fn new(syntax: &SyntaxReference) -> Self {
        Self {
            old: Side::new(syntax),
            new: Side::new(syntax),
        }
    }

    /// Advance the sides `line` exists on, and hand back its spans.
    ///
    /// Context advances both and is drawn from the working-tree side, which is
    /// the side the reader is looking at. That second parse is the cost the
    /// module header names, and it buys a removal being coloured by the file it
    /// came out of rather than by the file that replaced it.
    fn parse(
        &mut self,
        line: &Line,
        buf: &mut String,
        syntaxes: &SyntaxSet,
        table: &[(Scope, Class)],
    ) -> Vec<Span> {
        // `load_defaults_newlines` is the dump syntect supports; its
        // no-newline twin is documented as unreliable because grammars anchor
        // on end of line. The core strips line endings, so one is put back
        // here, into a buffer the hunk reuses rather than an allocation per
        // line.
        buf.clear();
        buf.push_str(&line.text);
        buf.push('\n');

        match line.kind {
            LineKind::Removed => self.old.spans(buf, line, syntaxes, table),
            LineKind::Added => self.new.spans(buf, line, syntaxes, table),
            LineKind::Context => {
                self.old.advance(buf, syntaxes);
                self.new.spans(buf, line, syntaxes, table)
            }
        }
    }
}

/// One hunk's parse, kept between frames.
struct Entry {
    path: String,
    /// Which hunk of that file, in file order.
    ordinal: usize,
    /// Content digest of the hunk this parse describes.
    digest: u64,
    /// Digest of every whole stride of this hunk, deepest last.
    ///
    /// Held for the content currently cached, so the next frame can find how
    /// far down the two agree without re-reading what it already parsed.
    marks: Vec<u64>,
    /// Parse positions this entry can rewind to, one per whole stride
    /// **parsed**, deepest last.
    ///
    /// Indexed to match `marks`: entry `i` is the state after exactly
    /// `(i + 1) * CHECKPOINT_STRIDE` lines, and `marks[i]` is the digest of
    /// those same lines. That pairing is what makes a rewind exact, and it is
    /// why the digest is not stored here as well: one copy cannot disagree with
    /// itself.
    ///
    /// Shorter than `marks` whenever the reader has not scrolled to the bottom
    /// of the hunk, which is almost always.
    checkpoints: Vec<Sides>,
    /// Whether the frame in progress has claimed it. See [`Highlighter::sweep`].
    live: bool,
    /// `None` when nothing recognises the file type, which is not an error.
    sides: Option<Sides>,
    /// Spans per display line, filled forward on demand and never rebuilt.
    lines: Vec<Vec<Span>>,
    /// Scratch for the newline the grammars expect. One per hunk, not per line.
    buf: String,
}

impl Entry {
    fn new(path: &str, ordinal: usize, content: Content, syntaxes: &SyntaxSet) -> Self {
        Self {
            path: path.to_owned(),
            ordinal,
            digest: content.digest,
            marks: content.marks,
            checkpoints: Vec::new(),
            live: true,
            sides: syntax_for(syntaxes, path).map(Sides::new),
            lines: Vec::new(),
            buf: String::new(),
        }
    }

    /// Whether this entry is the parse of that hunk.
    ///
    /// The cache key, in one place. It is a **compound** key, and both halves are
    /// load bearing: `ordinal` alone collides across files and `path` alone
    /// collides across a file's hunks, either of which shows a reader the colours
    /// of a hunk they are not looking at, which is the failure
    /// [`Pass::spans`] warns about. Written out at each of its three call sites
    /// it was three places to find the day the key gains a term.
    fn is(&self, path: &str, ordinal: usize) -> bool {
        self.ordinal == ordinal && self.path == path
    }

    /// Keep as much of this parse as `content` still agrees with, and report
    /// whether anything survived.
    ///
    /// The alternative is what this used to do: throw the whole parse away and
    /// start at line zero. That made a frame cost the reader's scroll depth
    /// rather than what it drew, on every frame, for as long as the file being
    /// read was the file being written. See [`CHECKPOINT_STRIDE`].
    ///
    /// Exact, not approximate. Two hunks whose first *n* lines hash the same
    /// parse those lines the same way, because a line's scopes depend only on
    /// what came before it, so the spans above the deepest agreeing checkpoint
    /// are still the right answer.
    fn rewind(&mut self, content: Content) -> bool {
        // How many whole strides of the new content match what is cached, and
        // of those, how many were actually parsed deep enough to have a
        // position to resume from.
        let agreed = self
            .marks
            .iter()
            .zip(&content.marks)
            .take_while(|(cached, fresh)| cached == fresh)
            .count();
        let usable = agreed.min(self.checkpoints.len());
        if usable == 0 {
            return false;
        }

        self.checkpoints.truncate(usable);
        self.lines.truncate(usable * CHECKPOINT_STRIDE);
        // Cloned rather than taken: the reader may sit here for many frames, and
        // each one needs to rewind to this same position again.
        self.sides = Some(self.checkpoints[usable - 1].clone());
        self.digest = content.digest;
        self.marks = content.marks;
        true
    }

    /// Parse forward until line `index` has spans.
    ///
    /// Forward only, and never re-entered for a line already done, which is what
    /// makes scrolling down a thousand-line hunk cost a screenful per frame
    /// instead of the hunk.
    fn fill_to(
        &mut self,
        index: usize,
        hunk: &Hunk,
        syntaxes: &SyntaxSet,
        table: &[(Scope, Class)],
        stats: &mut HighlightStats,
    ) {
        while self.lines.len() <= index {
            // A whole stride has been parsed, so freeze where it left off.
            // Taken here rather than after the last line of the stride so that a
            // checkpoint is always on a clean line boundary, and guarded on the
            // count so re-entering `fill_to` at the same depth cannot push a
            // second copy of a position already held.
            let done = self.lines.len();
            if done > 0
                && done % CHECKPOINT_STRIDE == 0
                && self.checkpoints.len() < done / CHECKPOINT_STRIDE
                && let Some(sides) = &self.sides
            {
                self.checkpoints.push(sides.clone());
            }

            let line = &hunk.lines[self.lines.len()];
            let spans = match &mut self.sides {
                Some(sides) => {
                    stats.lines += 1;
                    stats.bytes += line.text.len() as u64;
                    sides.parse(line, &mut self.buf, syntaxes, table)
                }
                // Nothing recognises the file type, so every byte is plain and
                // nothing is parsed. Counted nowhere for that reason.
                None => plain(line.text.len()),
            };
            self.lines.push(spans);
        }
    }
}

/// The syntax classes of whatever is on screen, kept between frames.
///
/// Created once and driven per frame through [`Highlighter::pass`], which is the
/// only way to reach a hunk's spans and which sweeps the cache when it drops.
///
/// ```no_run
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let worktree = vigia_core::Worktree::discover(".")?;
/// let mut frame = worktree.frame();
/// let mut highlighter = vigia_core::Highlighter::new();
/// frame.advance()?;
///
/// let (_, diff) = frame.diff(0)?;
/// let path = diff.path.clone();
/// let hunk = diff.hunks[0].clone();
///
/// let mut pass = highlighter.pass();
/// for i in 0..hunk.lines.len() {
///     println!("{:?}", pass.spans(&path, 0, &hunk, i));
/// }
/// # Ok(())
/// # }
/// ```
pub struct Highlighter {
    /// The bundled grammars, shared rather than owned outright.
    ///
    /// `Arc` because a `SyntaxSet` compiles its `fancy_regex` patterns **lazily,
    /// on first use, into `once_cell::sync::OnceCell`s it owns**. That makes the
    /// compiled form a property of the set rather than of a parse, so a second
    /// thread holding this same set can pay that cost where nothing is waiting
    /// on it and the frame path gets the result for free. A clone of the set
    /// instead of a handle to it would compile everything twice and share
    /// nothing. See [`warm`].
    ///
    /// Costs one atomic refcount. `SyntaxSet` is `Send + Sync`, which the
    /// **compiler proves** by accepting this `Arc` across the `thread::spawn` in
    /// [`Highlighter::warm_ahead`]; the arrangement is void without it, and a
    /// test asserting it would assert what the build already refuses to compile
    /// without.
    syntaxes: Arc<SyntaxSet>,
    /// [`CLASSES`] resolved once, because a [`Scope`] is an interned atom and
    /// building one from a string takes a lock on syntect's global repository.
    table: Vec<(Scope, Class)>,
    /// A `Vec` rather than a map, and that is a consequence of the viewport
    /// bound rather than an oversight. Sweeping every frame keeps this to the
    /// hunks one screen can show, which is a few dozen at the very most, so a
    /// linear scan is faster than hashing a key and is one less thing to get
    /// wrong.
    entries: Vec<Entry>,
    /// Hunks that have left the screen, newest last, capped at
    /// [`RETAINED_HUNKS`].
    ///
    /// **Why a monitor keeps anything it is not drawing.** Highlighting is
    /// forward-only, so a hunk entered at its *bottom* costs the whole walk above
    /// the visible row. Dropping a parse the moment it scrolls off means a reader
    /// who scrolls back over ground they just read pays that walk again, and
    /// measured over wide-character content that is **26.39ms** against a 16ms
    /// budget, once per file, with the answer sitting in memory a frame earlier
    /// ([#45](https://github.com/breferrari/vigia/issues/45)).
    ///
    /// This is a queue rather than a second `entries`, and the difference is the
    /// bound. Eviction order *is* the queue's order, so nothing has to be sorted
    /// or timestamped on the frame path, and the cache stays what one screen can
    /// show plus a constant. I3 is a claim about a process left open for days:
    /// a bigger constant is a higher plateau, which drift cannot see, and only an
    /// unbounded one would be a leak.
    retired: VecDeque<Entry>,
    stats: HighlightStats,
}

impl Highlighter {
    /// Load the bundled grammars and start with an empty cache.
    ///
    /// Costs 318µs measured in release, which is why it is done up front rather
    /// than behind a lazy initialiser: I7 gives startup 50ms, and hiding this
    /// behind first use would only move it onto the first frame that draws.
    ///
    /// **Loading a grammar and compiling one are different costs, and only the
    /// small one happens here.** `syntect` defers every pattern to
    /// `fancy_regex` on first use, so the first parse under a grammar costs
    /// **74-362ms** where this costs microseconds: Rust 93.11ms, Python 89.05ms,
    /// JavaScript 103.27ms, Go 74.14ms and Markdown 361.74ms, measured in
    /// release on the reference machine. That is the whole of
    /// [#51](https://github.com/breferrari/vigia/issues/51), it is why the
    /// shell's first frame draws plain, and it is what `warm` exists to move
    /// off the path a reader is waiting on.
    pub fn new() -> Self {
        Self {
            syntaxes: Arc::new(SyntaxSet::load_defaults_newlines()),
            table: CLASSES
                .iter()
                .filter_map(|(prefix, class)| Some((Scope::new(prefix).ok()?, *class)))
                .collect(),
            entries: Vec::new(),
            retired: VecDeque::with_capacity(RETAINED_HUNKS),
            stats: HighlightStats::default(),
        }
    }

    /// Compile grammars ahead of the reader, on a thread, and report how many
    /// files it managed.
    ///
    /// **Best effort, and it upholds nothing.** Winning the race makes a later
    /// frame cheaper; losing it costs only the work, because the frame path has
    /// no idea this exists. A frame that reaches a pattern this thread is
    /// mid-compile does *wait* on it, since `OnceCell::get_or_init` blocks, but
    /// it waits for a compile it would otherwise have paid itself, so the total
    /// is the same or better and never worse. That is the design rather than a
    /// weakness: `warm` explains why a *guarantee* here is not available at
    /// any price, and a frame path that believed one would be acting on a claim
    /// that is false.
    ///
    /// What it buys is the case a reader actually meets: a diff of several files
    /// where the second one entered costs 41ms under Rust and 95ms under
    /// Markdown for patterns the first file never reached. Those are ruled cold
    /// by `SPEC.md` §7 and are still the worst frames in a session.
    ///
    /// Reads raw bytes rather than going through the clean filter, because what
    /// is wanted is representative *text* rather than a faithful diff side; a
    /// CRLF file compiles the same patterns either way.
    ///
    /// The handle is returned rather than kept so a test can join it and assert
    /// the policy; dropping it detaches, which is what [`crate::Highlighter`]'s
    /// caller does. The thread ends on its own and holds nothing but an `Arc` to
    /// the grammars.
    ///
    /// Bounded in every direction it can run away in: [`WARM_FILES`] paths
    /// considered, [`WARM_TOTAL`] files parsed in all, [`WARM_PER_GRAMMAR`] of
    /// them per language, and [`WARM_BYTES`] read from each. A monitor is left
    /// open for days and I3 is what would notice an unbounded sweep of a
    /// worktree.
    ///
    /// **A panic here takes the process with it**, because the workspace builds
    /// with `panic = "abort"`, so `catch_unwind` is not available to make this
    /// thread as detachable as its "upholds nothing" reads. What could panic is
    /// `syntect`'s own `expect` on a pre-tested pattern and its poisonable
    /// global scope repository, neither of which this code can guard. Recorded
    /// rather than defended against, because the honest options are a spec
    /// change to the panic strategy or nothing.
    ///
    /// The per-grammar cap is checked **before** the read, so a run over a
    /// single-language changed set does three reads rather than sixty-four.
    pub fn warm_ahead(
        &self,
        root: std::path::PathBuf,
        paths: Vec<String>,
    ) -> std::thread::JoinHandle<usize> {
        let syntaxes = Arc::clone(&self.syntaxes);
        std::thread::spawn(move || {
            // Resolved once: every path below is checked against it, and a root
            // that cannot be resolved is a worktree that has gone away, which is
            // nothing to warm rather than something to guess at.
            let Ok(canonical_root) = std::fs::canonicalize(&root) else {
                return 0;
            };
            let mut warmed = 0usize;
            let mut per_grammar: HashMap<Scope, usize> = HashMap::new();
            for path in paths.into_iter().take(WARM_FILES) {
                // **The total, which the per-grammar cap does not bound.** A
                // polyglot changed set has as many budgets as it has languages:
                // fifty distinct extensions warmed forty-three files in
                // **3.93s** of held core before this line existed, against the
                // 1.053s worst case the per-grammar cap was reasoned about with.
                // No single-language fixture can see that, which is `SPEC.md`
                // §7's ASCII-fixture rule one axis over.
                if warmed >= WARM_TOTAL {
                    break;
                }

                // Repository-relative, and refused otherwise. Status yields
                // relative paths so this is unreachable from the shipped caller,
                // but `warm_ahead` is public on a public type and `PathBuf::join`
                // silently *discards* the root for an absolute path, so a caller
                // one `Vec<String>` away from wrong would read anywhere on the
                // disk.
                //
                // **A whitelist, because the blacklist it replaced had three
                // holes on Windows.** `Path::is_absolute` there requires a prefix
                // *and* a root, so `C:relative.rs`, `\\dir\\file.rs` and
                // `/dir/file.rs` all passed it while `join` still discarded the
                // worktree. Verified against the shipped `warm_ahead`: all three
                // read the bait file, and the gate covering this was green only
                // because it happened to use the two spellings the blacklist did
                // catch. Naming what a path may contain has no such holes.
                if !std::path::Path::new(&path).components().all(|c| {
                    matches!(
                        c,
                        std::path::Component::Normal(_) | std::path::Component::CurDir
                    )
                }) {
                    continue;
                }

                // Looked up before anything is read, which is what makes the
                // per-grammar cap save the I/O and not merely the parse. A path
                // with no grammar is skipped here rather than read and thrown
                // away, and it is the same answer `syntax_for` gives the frame
                // path for a file type nothing recognises.
                // Keyed on the grammar's `Scope`, which is what `syntect` itself
                // treats as a syntax's identity and is a `Copy` bit-packed atom.
                // The `name` is a display string, so keying on it would allocate
                // per path including the ones the cap is about to skip.
                let Some(grammar) = syntax_for(&syntaxes, &path).map(|s| s.scope) else {
                    continue;
                };
                let seen = per_grammar.entry(grammar).or_insert(0);
                if *seen >= WARM_PER_GRAMMAR {
                    continue;
                }

                // **And where it actually lands, which the component check
                // cannot know.** That one is lexical, so it stops `..` and every
                // spelling of a rooted path and stops nothing that goes through
                // a *link*: a symlinked directory inside the worktree reads
                // wherever it points, because `fs::read` follows one. That is
                // the OS behaviour rather than a policy this crate holds, and
                // `Worktree::read_worktree` deliberately does **not** follow a
                // link since [#15](https://github.com/breferrari/vigia/issues/15),
                // so the warmer cannot borrow its answer. Both
                // checks are wanted rather than either: resolving alone would
                // accept a `..` that happens to stay inside, and the lexical one
                // alone leaves the claim these docs make untrue.
                //
                // Costs one `canonicalize` per candidate, bounded by
                // [`WARM_TOTAL`], against a compile of 74-362ms. A path that
                // cannot be resolved has vanished, which is the `continue` the
                // open below would have taken anyway.
                let Ok(target) = std::fs::canonicalize(root.join(&path)) else {
                    continue;
                };
                if !target.starts_with(&canonical_root) {
                    continue;
                }

                // Bounded at the read rather than after it: see `WARM_BYTES`.
                // A file that vanished between status naming it and this thread
                // reaching it is ordinary beside an agent, and so is one that is
                // not text; both mean there is nothing here to compile with.
                let Ok(file) = std::fs::File::open(&target) else {
                    continue;
                };
                let mut buf = Vec::with_capacity(WARM_BYTES);
                if file.take(WARM_BYTES as u64).read_to_end(&mut buf).is_err() {
                    continue;
                }
                // A bounded read lands mid-codepoint on any file that is not
                // ASCII, so the tail is trimmed to the last complete character
                // rather than the read being widened to avoid it. `valid_up_to`
                // is by construction the end of a run of valid UTF-8, so the
                // inner call cannot fail; written as a fallback rather than a
                // match arm nothing could ever reach.
                let text = std::str::from_utf8(&buf)
                    .unwrap_or_else(|e| std::str::from_utf8(&buf[..e.valid_up_to()]).unwrap_or(""));

                // Nothing compiled means nothing spent, which is the rule a
                // vanished path already follows. A file that is not text at all
                // — a UTF-16 BOM is the ordinary case — trims to the empty
                // string, parses zero lines, and would otherwise burn one of
                // three per-grammar slots and be counted as a warm.
                if text.is_empty() {
                    continue;
                }

                warm(&syntaxes, &path, text);
                *seen += 1;
                warmed += 1;
            }
            warmed
        })
    }

    /// Begin a frame, and hand back the only thing that can ask for spans.
    ///
    /// The bound is the guard, not a convention. Sweeping is *the* I3 claim for
    /// this module, and the first version of it left the two halves as public
    /// calls a caller had to bracket by hand: a mutation deleting the sweep left
    /// the entire suite green while the cache grew by everything ever scrolled
    /// past. Two of the five call sites in that same commit had already forgotten
    /// it.
    ///
    /// So the sweep runs in [`Pass`]'s `Drop` and cannot be skipped, which is the
    /// shape `Session` already uses for I8 in the shell: *"there is deliberately
    /// no way to restore early and keep drawing"*. It also means a `?` between
    /// the first span and the last still leaves the cache bounded, so a caller
    /// needs no second function to make its error path safe.
    pub fn pass(&mut self) -> Pass<'_> {
        for entry in &mut self.entries {
            entry.live = false;
        }
        Pass { highlighter: self }
    }

    /// Retire every hunk the pass did not draw, and drop what will not fit.
    ///
    /// Two stages rather than one, and only the second is an eviction. A hunk
    /// that leaves the screen goes to the back of [`Self::retired`]; a hunk
    /// pushed out of *that* is gone and is what `evicted` counts. Keeping the
    /// counter on the second stage is deliberate: it is the number `SPEC.md` §7's
    /// "a bound is only evidence when something reached it" rule is asserted on,
    /// and counting a retirement there would report the queue turning over as
    /// though the cache were being emptied.
    fn sweep(&mut self) {
        // Destructured for the reason `spans` is: two fields of one struct are
        // written at once, and through `&mut self` the borrow checker sees one
        // whole thing.
        let Self {
            entries,
            retired,
            stats,
            ..
        } = self;

        // An index walk rather than `Vec::extract_if`, which would say this in
        // one line: that is stable since 1.87 and the workspace declares 1.85,
        // and correcting the manifest is a toolchain decision rather than
        // something to slip into a rendering fix. `Vec::remove` shifts the tail
        // per retirement, which is quadratic in principle and a few thousand
        // element moves at the very worst here, because `entries` is bounded by
        // what one screen can show.
        let mut at = 0;
        while at < entries.len() {
            if entries[at].live {
                at += 1;
            } else {
                retired.push_back(entries.remove(at));
            }
        }
        while retired.len() > RETAINED_HUNKS {
            retired.pop_front();
            stats.evicted += 1;
        }
    }

    /// Take a retired hunk back, if this is one the reader has come back to.
    ///
    /// Linear over a queue of [`RETAINED_HUNKS`], for the reason [`Self::entries`]
    /// is a `Vec`: the set is a handful and a scan beats a hash. Called only on a
    /// miss, so at most once per hunk per frame rather than once per row.
    fn recover(&mut self, path: &str, ordinal: usize) -> Option<Entry> {
        let at = self
            .retired
            .iter()
            .position(|entry| entry.is(path, ordinal))?;
        self.retired.remove(at)
    }

    fn spans(&mut self, path: &str, ordinal: usize, hunk: &Hunk, index: usize) -> &[Span] {
        // One scan, and the miss is where the retired queue is consulted, so a
        // hunk the reader has scrolled back to lands in `entries` and everything
        // below sees one cache rather than two. Whether its content is still
        // current is then decided exactly as it is for an entry that never left:
        // by digest, and by rewinding when it differs. Coming back to a hunk
        // whose file has been rewritten meanwhile is therefore correct rather
        // than merely fast.
        //
        // Written as one `position` rather than a `contains`-then-`position`
        // pair. This runs **once per drawn row**, so a second scan of the cache
        // here is a second scan per row per frame, on the path I9 gates, and it
        // would be spent against the same `Vec` whose choice over a map is
        // justified two doc comments up by there being only one scan.
        let found = match self
            .entries
            .iter()
            .position(|entry| entry.is(path, ordinal))
        {
            Some(slot) => Some(slot),
            None => self.recover(path, ordinal).map(|entry| {
                self.entries.push(entry);
                self.entries.len() - 1
            }),
        };

        // Destructured so the syntax set can be read while one entry and the
        // counters are written. Through `&mut self` alone the borrow checker
        // sees one whole thing.
        let Self {
            syntaxes,
            table,
            entries,
            stats,
            ..
        } = self;

        let slot = match found {
            // Already claimed this frame, so its content has been checked
            // already. This is what keeps the digest to once per hunk per frame
            // rather than once per line.
            Some(slot) if entries[slot].live => slot,
            Some(slot) => {
                let content = content_of(hunk);
                if entries[slot].digest == content.digest {
                    stats.reused += 1;
                    entries[slot].live = true;
                } else {
                    stats.parsed += 1;
                    // Rewind to the deepest position the new content still
                    // agrees with, and only start over when there is none.
                    if !entries[slot].rewind(content.clone()) {
                        entries[slot] = Entry::new(path, ordinal, content, syntaxes);
                    }
                    entries[slot].live = true;
                }
                slot
            }
            None => {
                stats.parsed += 1;
                entries.push(Entry::new(path, ordinal, content_of(hunk), syntaxes));
                entries.len() - 1
            }
        };

        let entry = &mut entries[slot];
        entry.fill_to(index, hunk, syntaxes, table, stats);
        &entry.lines[index]
    }

    /// Counters for what this highlighter has done.
    pub fn stats(&self) -> HighlightStats {
        self.stats
    }

    /// Hunks currently held between frames.
    ///
    /// At most what one screen can show, plus [`RETAINED_HUNKS`]. I3 is a claim
    /// about a process that runs for days, so this is the number that says the
    /// cache is bounded by the viewport rather than by the session.
    ///
    /// The retired queue is counted here rather than reported separately, and
    /// that is the honest direction: a caller asking what the cache holds is
    /// asking what it costs, and a parse kept for a reader who might scroll back
    /// occupies exactly as much memory as one being drawn.
    pub fn tracked(&self) -> usize {
        self.entries.len() + self.retired.len()
    }
}

/// One frame's worth of highlighting, which sweeps the cache when it is dropped.
///
/// Created by [`Highlighter::pass`] and the only way to reach a hunk's spans. See
/// that method for why the bound is a guard rather than a pair of calls.
pub struct Pass<'h> {
    highlighter: &'h mut Highlighter,
}

impl Pass<'_> {
    /// Spans for display line `index` of `hunk`, which is hunk `ordinal` of
    /// `path`.
    ///
    /// Reuses the previous frame's parse when the hunk's content is unchanged,
    /// rewinds to the deepest position it still agrees with when it is not, and
    /// parses forward from there. Content, not identity: a diff recomputed inside
    /// the settle margin holds the same hunks, and treating those as new would
    /// re-highlight files nobody edited.
    ///
    /// **`ordinal` is the hunk's position in the file, not in the window.** It is
    /// half of the cache key, so a caller that renumbered hunks per screen would
    /// hand the same key to different content every time the view scrolled, and
    /// the reader would see the colours of a hunk they are not looking at.
    ///
    /// # Panics
    ///
    /// If `index` is past the end of `hunk.lines`, the same way indexing a slice
    /// does, and for the same reason [`Frame::diff`](crate::Frame::diff) panics
    /// on a stale index: a caller has to be walking a hunk it holds, and a
    /// lenient accessor would turn that bug into a silently uncoloured row.
    pub fn spans(&mut self, path: &str, ordinal: usize, hunk: &Hunk, index: usize) -> &[Span] {
        self.highlighter.spans(path, ordinal, hunk, index)
    }

    /// Counters for what the highlighter has done, mid-pass.
    pub fn stats(&self) -> HighlightStats {
        self.highlighter.stats()
    }
}

impl Drop for Pass<'_> {
    fn drop(&mut self) {
        self.highlighter.sweep();
    }
}

impl Default for Highlighter {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Highlighter {
    /// Hand written because the bundled grammars are seventy-five syntaxes of
    /// compiled regex, and a derived `Debug` would put all of them in whatever
    /// this is nested inside.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Highlighter")
            .field("tracked", &self.entries.len())
            .field("stats", &self.stats)
            .finish_non_exhaustive()
    }
}

/// One `Plain` span covering `len` bytes, or none at all for an empty line.
///
/// Never a zero-length span: the contract is that spans sum to the line, and a
/// line of no bytes is covered by no spans.
fn plain(len: usize) -> Vec<Span> {
    if len == 0 {
        Vec::new()
    } else {
        vec![Span {
            len,
            class: Class::Plain,
        }]
    }
}

/// Paths [`Highlighter::warm_ahead`] will consider, at most.
///
/// The outer bound, and it counts paths *offered* rather than files read: a run
/// where the first sixty-four have all vanished warms nothing and stops, which
/// is the bounded answer. What decides how much work actually happens is
/// [`WARM_PER_GRAMMAR`], not this.
pub const WARM_FILES: usize = 64;

/// Files [`Highlighter::warm_ahead`] will parse **per grammar**.
///
/// **The bound that matters, because the benefit is per grammar and the cost is
/// per file.** Compiling a grammar is one file's work; every later file of the
/// same language costs a flat parse and buys only the decaying residual, and a
/// changed set is usually one language. Measured in release on real source
/// truncated the way this thread truncates: 1 file 43.4ms, 2 files 62.7ms, 8
/// files 154.9ms, 64 files **1.053s**. So a file cap alone spends about
/// **96%** of a full run re-parsing under a grammar already compiled, while
/// holding a core and touching `syntect`'s global scope-repository mutex next to
/// the frame path.
///
/// Three, because the residual decays rather than vanishing: the first file
/// compiles the grammar and the next two reach constructs it missed (a sibling
/// `.rs` file still cost 41.41ms after one). Past that the curve is flat and the
/// contention is not.
pub const WARM_PER_GRAMMAR: usize = 3;

/// Files [`Highlighter::warm_ahead`] will parse in total, whatever the mix.
///
/// **The per-grammar cap is per grammar, so a polyglot tree has as many budgets
/// as it has languages.** Fifty changed files across fifty extensions warmed
/// forty-three of them in **3.93s** of held core, against the 1.053s worst case
/// [`WARM_PER_GRAMMAR`] was reasoned about; the ceiling without this is
/// [`WARM_FILES`] compiles, five to six seconds. Twelve is four languages fully
/// warmed, which covers a normal repository, and it is the number that makes the
/// thread's cost bounded rather than merely shaped.
pub const WARM_TOTAL: usize = 12;

/// Bytes of each file [`Highlighter::warm_ahead`] will read and parse, at most.
///
/// Enough to reach the constructs a grammar is expensive for (strings, comments,
/// nesting) without walking a generated file to its end.
///
/// **A bound on the read, not only on the parse**, and the distinction cost a
/// rewrite: reading whole and truncating after is `read_to_string` on whatever
/// the agent in the other pane happened to change, which for a lockfile, a
/// minified bundle or a dataset is the whole thing. Measured on a 32.4 MB file
/// with the page cache warm, **8.08ms against 0.166ms** for a bounded read, and
/// far worse cold — plus a `String` the size of the largest changed file, which
/// is an RSS spike I3 has no reason to absorb.
pub const WARM_BYTES: usize = 64 * 1024;

/// Compile the patterns `text` reaches under `path`'s grammar, so a later frame
/// drawing that content does not.
///
/// A free function over `&SyntaxSet` rather than a method, because the caller is
/// on another thread: it holds an [`Highlighter::warm_ahead`] handle and the
/// compiled patterns land in the set both of them share. Nothing is returned and
/// nothing is recorded, which is deliberate and is the whole subtlety here.
///
/// **There is no such thing as a warm grammar.** `fancy_regex` compiles per
/// *pattern*, not per grammar, so warming on one file leaves a *different* file
/// of the same language still paying. Measured in release, fresh `SyntaxSet` per
/// case, warming on one document and then parsing another of the same language:
///
/// | | cold | after warming on a sibling | residual |
/// |---|---|---|---|
/// | `.rs` | 160.58ms | 120.23ms | **41.41ms** |
/// | `.md` | 614.96ms | 583.91ms | **95.04ms** |
/// | `.html` | 240.22ms | 38.88ms | **201.20ms** |
/// | `.go` | 78.39ms | 93.74ms | 2.50ms |
///
/// A synthetic sample is worse than a real sibling (114ms residual on Markdown,
/// 200ms on HTML), and a bare newline worse again (37.90ms on Rust). So a
/// `warmed: HashSet<Grammar>` would be a **lie the frame path could act on**:
/// it would report Rust warm and let the next file pay 41ms against I9's 16ms.
/// This function therefore makes no claim, keeps no record, and callers must not
/// build one on top of it.
///
/// What it *is* good for is running ahead of a reader over the content they are
/// about to reach, where winning the race makes a later frame cheaper and losing
/// it costs nothing but the work. `SPEC.md` §7 already puts a first parse on the
/// cold path that I9 excludes by definition, so this narrows a ruled-cold cost
/// rather than upholding a budget.
///
/// A path with no grammar is a no-op, which is ordinary: it is the same answer
/// [`syntax_for`] gives the frame path for a file type nothing recognises.
///
/// Crate-private, and that is what keeps `syntect` out of `vigia`'s vocabulary:
/// `SPEC.md` §6 puts the grammars in this crate, so a `&SyntaxSet` in the public
/// API would make the shell name a type it has no dependency on.
/// [`Highlighter::warm_ahead`] is the way in.
fn warm(syntaxes: &SyntaxSet, path: &str, text: &str) {
    let Some(syntax) = syntax_for(syntaxes, path) else {
        return;
    };
    let mut state = ParseState::new(syntax);
    // `split_inclusive` keeps the trailing newline each line was stored with,
    // which is what `load_defaults_newlines` grammars expect; splitting it off
    // would parse every line as though the file ended there.
    for line in text.split_inclusive('\n') {
        // A grammar that fails on a line stops the warm rather than the process.
        // Nothing downstream depends on this having finished, by construction.
        if state.parse_line(line, syntaxes).is_err() {
            return;
        }
    }
}

/// The grammar for `path`, by extension and then by whole file name.
///
/// Whole name second because `Makefile`, `Dockerfile` and `.gitignore` have no
/// extension to look up. `None` is ordinary rather than an error: an unrecognised
/// file draws exactly as it did before there was highlighting at all, which
/// `SPEC.md` §11.1 rules.
fn syntax_for<'s>(syntaxes: &'s SyntaxSet, path: &str) -> Option<&'s SyntaxReference> {
    let path = Path::new(path);
    path.extension()
        .and_then(|ext| ext.to_str())
        .and_then(|ext| syntaxes.find_syntax_by_extension(ext))
        .or_else(|| {
            path.file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| syntaxes.find_syntax_by_extension(name))
        })
}

/// A hunk's content, hashed whole and at every stride boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Content {
    /// Digest of the whole hunk, which is what decides reuse.
    digest: u64,
    /// Digest of the first `(i + 1) * CHECKPOINT_STRIDE` lines, deepest last.
    marks: Vec<u64>,
}

/// Hash a hunk, keeping the running value at every stride boundary.
///
/// One walk rather than two. The whole-hunk digest answers "may this parse be
/// reused at all", and the marks answer "how much of it survives" when the
/// answer to the first is no. Computing the marks costs a `DefaultHasher` clone
/// per stride, which is a few words, against a walk of the hunk that was
/// happening anyway.
///
/// The **kind** is hashed alongside the text because the two sides parse
/// separately: the same string can be a removal in one frame and context in the
/// next, and the two get their state from different places. Hashing only the
/// text would serve one for the other.
///
/// Line numbers are deliberately left out. A hunk that moved is the same hunk to
/// look at, so it keeps its colours.
fn content_of(hunk: &Hunk) -> Content {
    let mut hasher = DefaultHasher::new();
    let mut marks = Vec::with_capacity(hunk.lines.len() / CHECKPOINT_STRIDE);
    for (at, line) in hunk.lines.iter().enumerate() {
        (line.kind as u8).hash(&mut hasher);
        line.text.hash(&mut hasher);
        if (at + 1) % CHECKPOINT_STRIDE == 0 {
            marks.push(hasher.clone().finish());
        }
    }
    Content {
        digest: hasher.finish(),
        marks,
    }
}

/// Append `len` bytes of `class`, merging into the run before it when they
/// agree.
///
/// Merging is not tidiness. A grammar emits a scope change at every token
/// boundary, so an unmerged line of ordinary code is a span per word, and the
/// renderer pays for each one.
fn push(spans: &mut Vec<Span>, len: usize, class: Class) {
    if len == 0 {
        return;
    }
    match spans.last_mut() {
        Some(last) if last.class == class => last.len += len,
        _ => spans.push(Span { len, class }),
    }
}

/// What the innermost scope on `stack` means.
///
/// Innermost outward, because a scope stack reads general to specific and the
/// specific end is the one that says what a token *is*: `source.rust` sits under
/// every line and would answer every question if it were consulted first.
fn classify(stack: &ScopeStack, table: &[(Scope, Class)]) -> Class {
    for scope in stack.as_slice().iter().rev() {
        for (prefix, class) in table {
            if prefix.is_prefix_of(*scope) {
                return *class;
            }
        }
    }
    Class::Plain
}

#[cfg(test)]
mod tests {
    //! The parts that are pure, tested as the pure things they are.
    //!
    //! The cache's own rule is not here. It is a claim about two consecutive
    //! frames over a real repository, so it is gated in
    //! `tests/budgets.rs` where the frames are.

    use super::*;

    fn line(kind: LineKind, text: &str) -> Line {
        Line {
            kind,
            text: text.to_owned(),
        }
    }

    fn hunk(lines: Vec<Line>) -> Hunk {
        Hunk {
            old_start: 1,
            old_lines: lines.len() as u32,
            new_start: 1,
            new_lines: lines.len() as u32,
            lines,
        }
    }

    /// Every span of every line, for a whole hunk of one file.
    fn spans_for(path: &str, hunk: &Hunk) -> Vec<Vec<Span>> {
        let mut highlighter = Highlighter::new();
        let mut pass = highlighter.pass();
        (0..hunk.lines.len())
            .map(|i| pass.spans(path, 0, hunk, i).to_vec())
            .collect()
    }

    /// The class of the run covering byte `at`.
    fn class_at(spans: &[Span], at: usize) -> Class {
        let mut start = 0;
        for span in spans {
            if at < start + span.len {
                return span.class;
            }
            start += span.len;
        }
        panic!("byte {at} is past the end of {spans:?}");
    }

    /// The whole contract [`Span`] documents, over content chosen to exercise
    /// every class the table can produce.
    #[test]
    fn spans_cover_a_line_exactly_and_never_reach_the_newline() {
        let texts = [
            "pub fn compute(path: String) -> u32 { 42 }",
            "    // an ordinary comment",
            "let s = \"a string\"; // and a trailing comment",
            "",
            "        ",
            "let n = 0xdead_beef;",
        ];
        let source = hunk(texts.iter().map(|t| line(LineKind::Added, t)).collect());

        for (spans, text) in spans_for("src/lib.rs", &source).iter().zip(texts) {
            let covered: usize = spans.iter().map(|span| span.len).sum();
            assert_eq!(
                covered,
                text.len(),
                "spans for {text:?} cover {covered} bytes of {}",
                text.len()
            );
            assert!(
                spans.iter().all(|span| span.len > 0),
                "a zero-length span in {spans:?} for {text:?}"
            );
        }
    }

    /// The table's order, which is the whole of its correctness.
    ///
    /// Every case here is a scope Sublime's Rust grammar really emits, taken by
    /// dumping the stack rather than guessed at, and each one is a pair the
    /// table would get wrong if its rows were reordered.
    #[test]
    fn the_scope_table_resolves_the_pairs_that_shadow_each_other() {
        let highlighter = Highlighter::new();
        let class_of = |scope: &str| {
            let mut stack = ScopeStack::new();
            stack.push(Scope::new("source.rust").expect("scope"));
            stack.push(Scope::new(scope).expect("scope"));
            classify(&stack, &highlighter.table)
        };

        // `fn` is scoped as a storage *type* and has to read as a keyword, while
        // `u32` is a storage type and has to read as one.
        assert_eq!(class_of("storage.type.function.rust"), Class::Keyword);
        assert_eq!(class_of("storage.type.rust"), Class::Type);
        // A numeric literal is a constant, so the specific row has to win.
        assert_eq!(
            class_of("constant.numeric.integer.decimal.rust"),
            Class::Number
        );
        assert_eq!(class_of("constant.language.rust"), Class::Constant);
        // A function's name is an entity name, so again the specific row first.
        assert_eq!(class_of("entity.name.function.rust"), Class::Function);
        assert_eq!(class_of("entity.name.struct.rust"), Class::Type);
        // `self` is a variable in the grammar and a keyword to a reader.
        assert_eq!(class_of("variable.language.rust"), Class::Keyword);
        assert_eq!(class_of("variable.parameter.rust"), Class::Variable);

        assert_eq!(class_of("storage.modifier.rust"), Class::Keyword);
        assert_eq!(class_of("comment.block.rust"), Class::Comment);
        assert_eq!(class_of("string.quoted.double.rust"), Class::String);
        // Nothing claims punctuation, and nothing should: a screen where every
        // brace is coloured is a screen with no emphasis left to spend.
        assert_eq!(
            class_of("punctuation.section.block.begin.rust"),
            Class::Plain
        );
        assert_eq!(class_of("meta.function.parameters.rust"), Class::Plain);
    }

    /// The scopes above, reached through a real parse rather than pushed by
    /// hand, because a table that is right about strings nothing produces is
    /// right about nothing.
    #[test]
    fn a_parsed_line_gets_the_classes_a_reader_expects() {
        let text = "pub fn compute(n: u32) -> u32 { 7 }";
        let source = hunk(vec![line(LineKind::Context, text)]);
        let spans = &spans_for("src/lib.rs", &source)[0];

        let at = |needle: &str| text.find(needle).expect("in the fixture");
        assert_eq!(class_at(spans, at("pub")), Class::Keyword);
        assert_eq!(class_at(spans, at("fn")), Class::Keyword);
        assert_eq!(class_at(spans, at("compute")), Class::Function);
        assert_eq!(class_at(spans, at("u32")), Class::Type);
        assert_eq!(class_at(spans, at("7")), Class::Number);
        assert_eq!(class_at(spans, at("{")), Class::Plain);
    }

    /// Merging, which is what keeps a line of ordinary code from becoming a span
    /// per token.
    #[test]
    fn adjacent_runs_of_one_class_become_one_span() {
        let source = hunk(vec![line(LineKind::Added, "// one two three four five")]);
        let spans = &spans_for("src/lib.rs", &source)[0];
        assert_eq!(
            spans,
            &[Span {
                len: "// one two three four five".len(),
                class: Class::Comment,
            }],
            "a comment came back as {} spans",
            spans.len()
        );
    }

    /// A file type nothing recognises, which `SPEC.md` §11.1 rules is ordinary.
    #[test]
    fn an_unrecognised_file_type_is_one_plain_span_and_parses_nothing() {
        let source = hunk(vec![line(LineKind::Added, "fn this is not any language")]);
        let mut highlighter = Highlighter::new();
        let spans = highlighter
            .pass()
            .spans("a/b.zzzznope", 0, &source, 0)
            .to_vec();

        assert_eq!(
            spans,
            vec![Span {
                len: "fn this is not any language".len(),
                class: Class::Plain,
            }]
        );
        assert_eq!(
            highlighter.stats().lines,
            0,
            "an unrecognised file type still went through the parser"
        );
        assert_eq!(highlighter.stats().bytes, 0);
    }

    /// A file with no extension at all, which is why the lookup has two steps.
    #[test]
    fn a_file_named_rather_than_extended_still_finds_its_grammar() {
        let syntaxes = SyntaxSet::load_defaults_newlines();
        assert!(syntax_for(&syntaxes, "src/lib.rs").is_some());
        assert!(syntax_for(&syntaxes, "Makefile").is_some());
        assert!(syntax_for(&syntaxes, "deep/nested/Makefile").is_some());
        assert!(syntax_for(&syntaxes, "src/no-such-thing.zzzznope").is_none());
    }

    /// Why the two sides are parsed apart.
    ///
    /// The removal opens a string and the addition opens it again. Through one
    /// state in display order the second quote *closes* what the first opened,
    /// and every line after it is coloured as if the string had ended. Parsed as
    /// two sides, each line is read against the file it belongs to.
    #[test]
    fn a_removal_does_not_leak_its_state_into_the_lines_after_it() {
        let source = hunk(vec![
            line(LineKind::Removed, "let s = \"before"),
            line(LineKind::Added, "let s = \"after"),
            line(LineKind::Added, "still inside the string\";"),
        ]);
        let spans = spans_for("src/lib.rs", &source);

        assert_eq!(
            class_at(&spans[2], 0),
            Class::String,
            "the line after an unterminated addition is not in the string, so \
             the removal's quote closed it"
        );
    }

    /// The other half of the same claim: context reaches both sides, so a
    /// removal is coloured by what came before it on its own side.
    #[test]
    fn a_removal_sees_the_context_above_it() {
        let source = hunk(vec![
            line(LineKind::Context, "/* a block comment opens here"),
            line(LineKind::Removed, "and this removal is inside it"),
            line(LineKind::Added, "and so is this addition"),
        ]);
        let spans = spans_for("src/lib.rs", &source);

        assert_eq!(class_at(&spans[1], 0), Class::Comment);
        assert_eq!(class_at(&spans[2], 0), Class::Comment);
    }

    /// The digest is what "the same hunk" means, so it has to see a kind change
    /// that leaves the text alone.
    #[test]
    fn the_digest_separates_hunks_that_differ_only_in_kind() {
        let text = "let value = 1;";
        let removed = hunk(vec![line(LineKind::Removed, text)]);
        let added = hunk(vec![line(LineKind::Added, text)]);
        let context = hunk(vec![line(LineKind::Context, text)]);

        assert_ne!(content_of(&removed).digest, content_of(&added).digest);
        assert_ne!(content_of(&added).digest, content_of(&context).digest);
        assert_eq!(
            content_of(&added).digest,
            content_of(&hunk(vec![line(LineKind::Added, text)])).digest
        );
    }

    /// Line numbers are not part of it, which is what lets a hunk that only
    /// moved keep its colours.
    #[test]
    fn the_digest_ignores_where_a_hunk_sits_in_the_file() {
        let lines = vec![line(LineKind::Added, "let value = 1;")];
        let here = hunk(lines.clone());
        let moved = Hunk {
            old_start: 900,
            new_start: 901,
            ..hunk(lines)
        };
        assert_eq!(content_of(&here).digest, content_of(&moved).digest);
    }
}
