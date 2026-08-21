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
//! | Load the bundled grammars (217 syntaxes, uncompressed dump) | 674µs, against I7's 50ms |
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
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::io::Read as _;
use std::path::Path;
use std::sync::{Arc, Mutex};

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
const CLASSES: [(&str, Class); 25] = [
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
    // A heading's text: Sublime's Markdown grammar scopes it
    // `entity.name.section` *inside* `markup.heading`, and innermost-first
    // classification reaches it before the markup row ever gets asked, so
    // without this row a heading reads as a type while its `#` reads as a
    // heading. Same class as `markup.heading`, deliberately: the two rows are
    // one element to the eye.
    ("entity.name.section", Class::Function),
    ("entity.name", Class::Type),
    // HTML attributes, CSS selectors, and their JSX/Vue/Svelte descendants.
    // Absent until #235, which is why an attribute drew plain in a covered
    // language. No shadowing with the `entity.name` rows above: prefix match
    // is per whole scope atom, so `entity.name` never claims `entity.other.*`.
    ("entity.other.attribute-name", Class::Variable),
    ("support.function", Class::Function),
    ("support.type", Class::Type),
    ("support.class", Class::Type),
    ("variable.language", Class::Keyword),
    ("variable", Class::Variable),
    // The markup family, absent entirely until #235: Markdown drew at 4.5%,
    // with headings, bold, code spans and links all plain, in the format
    // READMEs put in diffs constantly. `markup.list` is deliberately not
    // here: Sublime's Markdown grammar applies it as a meta scope across the
    // whole list item, so a row for it would paint every bullet's entire text
    // rather than a delimiter.
    ("markup.heading", Class::Function),
    ("markup.bold", Class::Keyword),
    ("markup.italic", Class::Keyword),
    ("markup.raw", Class::String),
    ("markup.underline.link", Class::Constant),
    ("markup.quote", Class::Comment),
    // The one deliberate `meta.*` row. A link's visible text carries only
    // `meta.link.inline.description` (probed against the shipped dump: the
    // `markup.underline.link` above covers the URL, not the words), so
    // without this the half of a link a reader actually reads stays plain.
    // Same class as the URL: one element to the eye.
    ("meta.link.inline.description", Class::Constant),
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
        // The embedded dump is built from the newlines variant (`xtask` merges
        // onto `two_face::syntax::extra_newlines` and loads extras with
        // lines-include-newline); the no-newline form is documented as
        // unreliable because grammars anchor on end of line. The core strips
        // line endings, so one is put back here, into a buffer the hunk reuses
        // rather than an allocation per line.
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
    /// Whether this entry is parsing, waiting on a warm, or will never parse.
    parse: Parse,
    /// Spans per display line, filled forward on demand and never rebuilt.
    lines: Vec<Vec<Span>>,
    /// Scratch for the newline the grammars expect. One per hunk, not per line.
    buf: String,
}

/// What an entry is doing about colour, which is three things and not two.
///
/// **It was two `Option`s, and the fourth combination they admit is the reason
/// this is an enum.** `sides: Option<Sides>` beside `deferred: Option<Scope>`
/// can spell *parsing **and** waiting on a warm*, which is meaningless, and the
/// only thing keeping it from being constructed was a paragraph on the field
/// saying so. A state that has to be argued for in prose is a state the type
/// should refuse, and the audit that found this said exactly that: the comment
/// is the evidence the enum was missing.
///
/// The distinction the prose was protecting is real and survives here as two
/// named variants: [`Self::Unsupported`] never changes and must never be asked
/// for again, while [`Self::Deferred`] is a file the reader *will* see in
/// colour, one warm from now.
enum Parse {
    /// Nothing in the dump recognises the file type, which is not an error and
    /// never becomes one. Every line is plain forever.
    Unsupported,
    /// The grammar is known and nothing has compiled it yet, so this draws plain
    /// and [`Highlighter::spans`] rebuilds the entry once the warmer has been
    /// over that scope.
    Deferred(Scope),
    /// Parsing, forward-only, from the position these sides hold.
    Ready(Sides),
}

impl Entry {
    /// Build the parse of one hunk, deferring when its grammar is uncompiled.
    ///
    /// `attempted` is the set [`Highlighter::attempted`] documents, or `None`
    /// for a highlighter built by [`Highlighter::eager`], which parses whatever
    /// it resolves.
    ///
    /// The lock is taken here rather than by the caller because this is the one
    /// place the answer is needed, and it runs **once per hunk built** rather
    /// than once per row: every row after a hunk's first takes the live fast
    /// path in [`Highlighter::spans`], which asks nothing.
    fn new(
        path: &str,
        ordinal: usize,
        content: Content,
        syntaxes: &SyntaxSet,
        first_line: Option<&str>,
        attempted: Option<&Mutex<HashSet<Scope>>>,
    ) -> Self {
        Self {
            path: path.to_owned(),
            ordinal,
            digest: content.digest,
            marks: content.marks,
            checkpoints: Vec::new(),
            live: true,
            // The one branch this whole change turns on. A grammar the warmer
            // has not run over is a grammar whose every pattern is still an
            // uncompiled `OnceCell`, so parsing here is the 124 to 695ms a cold
            // screenful measures, which is what the reader is waiting on.
            parse: match syntax_for(syntaxes, path, first_line) {
                None => Parse::Unsupported,
                Some(syntax) if !compiled(syntax.scope, attempted) => Parse::Deferred(syntax.scope),
                Some(syntax) => Parse::Ready(Sides::new(syntax)),
            },
            lines: Vec::new(),
            buf: String::new(),
        }
    }

    /// The grammar this entry is waiting on a warm for, if it is waiting.
    fn deferred(&self) -> Option<Scope> {
        match self.parse {
            Parse::Deferred(scope) => Some(scope),
            _ => None,
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
        self.parse = Parse::Ready(self.checkpoints[usable - 1].clone());
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
                && let Parse::Ready(sides) = &self.parse
            {
                self.checkpoints.push(sides.clone());
            }

            let line = &hunk.lines[self.lines.len()];
            let spans = match &mut self.parse {
                Parse::Ready(sides) => {
                    stats.lines += 1;
                    stats.bytes += line.text.len() as u64;
                    sides.parse(line, &mut self.buf, syntaxes, table)
                }
                // Either nothing recognises the file type, or its grammar is
                // uncompiled and a warm has been asked for. Every byte is plain
                // and nothing is parsed, counted nowhere for that reason.
                Parse::Unsupported | Parse::Deferred(_) => plain(line.text.len()),
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
///     println!("{:?}", pass.spans(&path, 0, &hunk, i, None));
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
    /// Grammars the warmer has run over, shared with every thread it spawns.
    ///
    /// **Read the claim carefully, because the obvious reading of it is the one
    /// [#51](https://github.com/breferrari/vigia/issues/51) was right to
    /// reject.** This does *not* say a grammar is warm. Compilation is per
    /// pattern, so no set can say that and [`warm`] explains why at length.
    /// Membership says only that the warmer has had its run over a file of this
    /// grammar, and it is the **absence** that the frame path acts on: a scope
    /// that is not in here has had no `ParseState` built for it by either of the
    /// two places in this crate that build one, so every pattern it owns is an
    /// uncompiled `OnceCell` and parsing it costs the whole cliff: 74-362ms of
    /// compile per grammar, which is 124 to 695ms measured over a screenful.
    /// That claim is exact, and the frame path only ever moves on it in the
    /// **safe** direction: draw plain and ask for a warm. Nothing here ever
    /// concludes that parsing will be cheap.
    ///
    /// **Written by the warmer for every path whose grammar resolved, whatever
    /// happened after the read.** A file that vanished between the demand and
    /// the open must not leave the frame path asking for it on every frame
    /// forever, which is a livelock with a wake attached; the honest fallback is
    /// that such a grammar is parsed cold once, which is exactly what shipped
    /// before this existed.
    ///
    /// `None` for a highlighter built by [`Highlighter::eager`].
    attempted: Option<Arc<Mutex<HashSet<Scope>>>>,
    /// Paths whose grammar was uncompiled during the pass in progress, one per
    /// grammar, in the order the frame reached them.
    ///
    /// Cleared by [`Highlighter::pass`], so it describes **this** frame and a
    /// caller cannot act on a demand the screen has moved past. Deduplicated by
    /// grammar rather than by path, because the warmer's whole benefit is per
    /// grammar and offering it eight files of one language would spend the
    /// budget [`WARM_PER_GRAMMAR`] exists to protect.
    wanted: Vec<String>,
    /// Scopes already on `wanted` this pass, so the dedup above costs no scan.
    demanded: HashSet<Scope>,
}

impl Highlighter {
    /// Load the bundled grammars and start with an empty cache.
    ///
    /// The grammars are the dump `xtask` builds and commits at
    /// `assets/syntaxes.bin` — `two-face`'s `fancy`-vetted packaging of `bat`'s
    /// curated set plus the locally vendored extras, which is the covered set
    /// `SPEC.md` §6 rules and [#235](https://github.com/breferrari/vigia/issues/235)
    /// decided. `from_binary` panics on a malformed dump, and deliberately so:
    /// the bytes are compiled into the binary, so a dump that cannot load is a
    /// build defect every test run catches, not a runtime condition to recover
    /// from.
    ///
    /// Loading is done up front rather than behind a lazy initialiser: I7
    /// gives startup 50ms, this costs **0.674ms** in release (best of 20; the
    /// old 75-syntax dump was 318µs), and hiding it behind first use would
    /// only move it onto the first frame that draws. The dump is
    /// **uncompressed**, and that is a measured decision rather than a
    /// default: the compressed form of the same set loads at **9.67ms**, a
    /// 14x, against 90KB saved in a binary whose size budget has an order of
    /// magnitude more room than its startup budget. `two-face` ships its own
    /// dumps uncompressed for the same reason.
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
    ///
    /// **A hunk whose grammar the warmer has not run over draws plain**, and
    /// [`Self::wanted`] is how the caller learns to ask for it. That is I7's
    /// rule one grammar over rather than a new one: I7 already says the first
    /// frame of a process draws without colour because a compile belongs behind
    /// a screen that has content on it, and
    /// [#129](https://github.com/breferrari/vigia/issues/129) is the same cost
    /// arriving mid-session, on a frame nobody asked for, because the agent in
    /// the other pane wrote a language this process had not met. Use
    /// [`Self::eager`] for a caller with nowhere to send a demand.
    pub fn new() -> Self {
        Self {
            attempted: Some(Arc::new(Mutex::new(HashSet::new()))),
            ..Self::eager()
        }
    }

    /// The same highlighter, parsing whatever it resolves, cold or not.
    ///
    /// **A caller with nowhere to send a demand, which is a real case rather
    /// than only a test one.** [`Self::new`]'s deferral is worth having exactly
    /// when something is going to call [`Self::warm_ahead`] and redraw; a
    /// consumer of this crate that highlights once and prints the answer would
    /// otherwise get plain text and no way to see why.
    ///
    /// It is also what the gates over colour use, for the reason
    /// `App::past_first_paint` exists in the shell: a test that wants colour out
    /// of a single pass would otherwise be measuring the one pass that
    /// deliberately does not parse.
    ///
    /// **Never reach for this in the shell.** It puts the 74-362ms compile back
    /// on a frame a reader is waiting on, and nothing would go red: the gate
    /// that would notice builds its own [`Self::new`].
    pub fn eager() -> Self {
        Self {
            syntaxes: Arc::new(
                syntect::dumps::from_uncompressed_data(include_bytes!("../assets/syntaxes.bin"))
                    .expect("the embedded dump deserialises"),
            ),
            table: CLASSES
                .iter()
                .filter_map(|(prefix, class)| Some((Scope::new(prefix).ok()?, *class)))
                .collect(),
            entries: Vec::new(),
            retired: VecDeque::with_capacity(RETAINED_HUNKS),
            stats: HighlightStats::default(),
            attempted: None,
            wanted: Vec::new(),
            demanded: HashSet::new(),
        }
    }

    /// Paths whose grammar nothing has compiled, one per grammar, as of the last
    /// pass.
    ///
    /// **The whole of the caller's obligation**, and it is a pull rather than a
    /// push on purpose: the shell asks once per painted frame, after the paint,
    /// so a demand can never be acted on for a screen that has already moved.
    /// Hand these to [`Self::warm_ahead`] with a sender and redraw when it
    /// fires. Ignoring them is legal and costs only colour.
    pub fn wanted(&self) -> &[String] {
        &self.wanted
    }

    /// Compile grammars ahead of the reader, on a thread, and report how many
    /// files it managed.
    ///
    /// **Still best effort about *how much* it compiles, and no longer best
    /// effort about *whether the frame path knows*.** Winning a race makes a
    /// later frame cheaper; a frame that reaches a pattern this thread is
    /// mid-compile does *wait* on it, since `OnceCell::get_or_init` blocks, but
    /// it waits for a compile it would otherwise have paid itself, so the total
    /// is the same or better and never worse. `warm` explains why a *guarantee*
    /// about a grammar's warmth is not available at any price, and that is
    /// unchanged.
    ///
    /// What changed with
    /// [#129](https://github.com/breferrari/vigia/issues/129) is the one thing
    /// that never needed a guarantee: this run records the grammars it had a go
    /// at, in the set `Highlighter::attempted` documents, and the frame path
    /// declines to parse under a grammar that is **not** in there. That claim is
    /// exact and the frame path moves on it only in the safe direction.
    ///
    /// What it buys is the frame the reader actually meets. Measured on the
    /// reference machine, release, one twenty-four line screenful under a
    /// grammar this process had never touched: **123.98ms cold against 2.40ms**
    /// once the warmer had read one real file of the same language, which is the
    /// floor for that content exactly. Markdown is 694.75ms against 90.65ms, and
    /// the cliff is flat in content size, which is what says it is a compile
    /// rather than a parse: a 594-byte screenful costs 631.46ms cold and 0.97ms
    /// warm.
    ///
    /// **A small synthetic sample is not enough**, and that is why this reads
    /// [`WARM_BYTES`] of a real file rather than parsing a fixture. Warming on a
    /// 2.5KB hand-written sample leaves a real residual: `.js` 80.49ms, `.html`
    /// 40.10ms, `.cpp` 37.55ms above floor.
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
    /// single-language changed set does three reads rather than sixty-four. It
    /// is checked again after it, because one resolution step reads the first
    /// line ([`CONTENT_SENSITIVE`]) and the grammar actually compiled is the
    /// one that must be charged: charging the pre-read answer spent
    /// TypeScript's budget on a Qt translation file's XML.
    ///
    /// `done` is signalled once, when the run ends, however it ended. It is what
    /// turns a warm into a **redraw**: the shell's loop is blocked on a channel,
    /// so a grammar that finished compiling changes nothing on screen until
    /// something wakes it. That send is not a timer and I1 does not reach it:
    /// I1's words are *no filesystem event and no git index change means no
    /// work*, and with nothing written there is nothing to warm and nothing to
    /// send. `None` for a caller that is going to join the handle instead.
    pub fn warm_ahead(
        &self,
        root: std::path::PathBuf,
        paths: Vec<String>,
        done: Option<Warmed>,
    ) -> std::thread::JoinHandle<WarmReport> {
        let syntaxes = Arc::clone(&self.syntaxes);
        let attempted = self.attempted.clone();
        std::thread::spawn(move || {
            let report = Self::warm_run(&syntaxes, attempted.as_deref(), &root, paths);
            // After the work rather than before it, so a wake means there is
            // something new to draw. Ignored on failure: the receiver having
            // gone is the shell shutting down, which is nothing to report to.
            if let Some(done) = done {
                done();
            }
            report
        })
    }

    /// Compile the grammars this repository **leads with**, on a thread.
    ///
    /// **The half of the fix that keeps the pane from flickering at all in the
    /// case being watched.** [`Self::warm_ahead`] over the changed set covers a
    /// tree somebody has already written to, and a monitor is very often opened
    /// on a clean one *before* the agent writes: the warmer then has nothing to
    /// warm, and the first write arrives cold. The index knows what this
    /// repository is made of before anything is touched.
    ///
    /// **Three grammars, and the bound is the whole ruling.** Warming the tail
    /// is not affordable: measured over this repository, warming ten grammars
    /// moves RSS from **6.73 MiB to 64.73 MiB**, Markdown alone accounting for
    /// +19 to +35 MiB and Rust +12.43 MiB, and that is memory spent on languages
    /// a session may never meet in a tool whose thesis is that it is cheap to
    /// leave open for days. Warming what a repository *leads* with is barely
    /// speculative by comparison: in a Rust repository the agent is near-certain
    /// to touch Rust, so the same megabytes are paid within seconds either way
    /// and this only moves them earlier.
    ///
    /// Its own thread rather than a second phase of [`Self::warm_ahead`],
    /// because the changed set is what is **on screen** and must not queue
    /// behind a sweep of the index. **At most two warms therefore run at once**,
    /// and which two is the point: the shell's `request_warm` holds the demand
    /// side to one at a time, and this is the other. Both are bounded by
    /// [`WARM_TOTAL`] files, so the ceiling is a constant rather than a race.
    ///
    /// Opens its own repository, for the reason the shell's watch thread does:
    /// `gix::Repository` is `Send` and not `Sync`, so a borrow of the frame
    /// path's cannot cross this boundary. That read is off the path I7 measures
    /// by construction, since nothing here runs before the first paint.
    pub fn warm_repository(
        &self,
        root: std::path::PathBuf,
        done: Option<Warmed>,
    ) -> std::thread::JoinHandle<WarmReport> {
        let syntaxes = Arc::clone(&self.syntaxes);
        let attempted = self.attempted.clone();
        std::thread::spawn(move || {
            let paths = leading_paths(&syntaxes, &root);
            let report = Self::warm_run(&syntaxes, attempted.as_deref(), &root, paths);
            if let Some(done) = done {
                done();
            }
            report
        })
    }

    /// One warm, on the thread its caller spawned.
    ///
    /// Lifted out of [`Self::warm_ahead`] when [`Self::warm_repository`]
    /// arrived, so the two entry points share **one** set of bounds. A second
    /// copy of the caps would be a second copy of the reasoning behind them, and
    /// the reasoning is the expensive part.
    ///
    /// An associated function rather than a method, because it runs on the
    /// spawned thread and a `&self` there would be a borrow of the highlighter
    /// the frame path is using.
    ///
    /// **It knows nothing about which grammars are worth warming**, and that is
    /// the seam: both callers hand it a path list they have already chosen, and
    /// what it owns is the four bounds below and nothing else. A
    /// distinct-grammar cap lived here briefly, so that the population sweep
    /// could stop after three; it moved to `leading_paths`, where the choosing
    /// happens, and took a `usize::MAX` sentinel with it.
    fn warm_run(
        syntaxes: &SyntaxSet,
        attempted: Option<&Mutex<HashSet<Scope>>>,
        root: &Path,
        paths: Vec<String>,
    ) -> WarmReport {
        // Resolved once: every path below is checked against it, and a root
        // that cannot be resolved is a worktree that has gone away, which is
        // nothing to warm rather than something to guess at.
        let Ok(canonical_root) = std::fs::canonicalize(root) else {
            return WarmReport::default();
        };
        let mut report = WarmReport::default();
        let mut per_grammar: HashMap<Scope, usize> = HashMap::new();
        for path in paths.into_iter().take(WARM_FILES) {
            // **The total, which the per-grammar cap does not bound.** A
            // polyglot changed set has as many budgets as it has languages:
            // fifty distinct extensions warmed forty-three files in
            // **3.93s** of held core before this line existed, against the
            // 1.053s worst case the per-grammar cap was reasoned about with.
            // No single-language fixture can see that, which is `SPEC.md`
            // §7's ASCII-fixture rule one axis over.
            if report.warmed >= WARM_TOTAL {
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
            // with no grammar at all is skipped here rather than read and
            // thrown away, and it is the same answer `syntax_for` gives the
            // frame path for a file type nothing recognises.
            //
            // **The cap is charged after the read, not here**, and the two
            // are different questions. This one is *is there anything to
            // compile*, which the path answers on its own. Charging needs
            // the grammar that will actually be compiled, and one of the
            // resolution steps reads the file's first line: a Qt `.ts`
            // translation file compiles XML while resolving to TypeScript
            // by extension, so charging here spent TypeScript's budget on
            // XML's work and could starve the real TypeScript file later in
            // the same run.
            //
            // **A miss here is not the end of the path's turn, and treating it
            // as one was an I1 breach.** An extensionless script's grammar is
            // named by its own first line, so `Entry::new` resolves it, defers,
            // and asks for a warm; this resolution has no first line to consult,
            // returned `None`, and skipped the file before [`Attempt`] existed
            // to mark anything. The frame then asked again on the next frame,
            // and every ask spawned a thread and sent a wake, on a tree nobody
            // was touching. So a path the extension cannot answer for falls
            // through to the read below rather than being skipped, which also
            // makes a shebang script warmable for the first time.
            let by_path = syntax_for(syntaxes, &path, None);

            // **The cap, cheaply, on the answer the path alone gives.**
            // This is what keeps a single-language changed set to three
            // reads: without it every file past the cap was read and
            // canonicalized before being discarded, which is the I/O
            // amplification the cap exists to prevent. It is sound for any
            // path whose grammar the first line cannot change; the ones it
            // can are named by [`CONTENT_SENSITIVE`] and pay the read to
            // find out.
            //
            // **The three-reads claim is about a set whose paths name their own
            // grammar, and one whose paths do not is the exception rather than a
            // hole in it.** A changed set of `LICENSE`, `AUTHORS` and `COPYING`
            // has nothing for this cap to consult, so each pays a read and finds
            // nothing, up to [`WARM_FILES`] of them. That is the price of
            // resolving a shebang script at all, it is bounded, and it is on a
            // thread with nothing waiting on it: the only caller that hands over
            // an unfiltered changed set is the shell's opening warm, since every
            // later one comes from [`Highlighter::wanted`], whose paths the
            // frame has already resolved a grammar for.
            //
            // **The cap is charged after the read, not here**, and the two
            // are different questions. This one is *is there anything to
            // compile*, which the path answers on its own. Charging needs
            // the grammar that will actually be compiled, and one of the
            // resolution steps reads the file's first line: a Qt `.ts`
            // translation file compiles XML while resolving to TypeScript
            // by extension, so charging here spent TypeScript's budget on
            // XML's work and could starve the real TypeScript file later in
            // the same run.
            if !content_sensitive(&path)
                && by_path.is_some_and(|by_path| {
                    per_grammar
                        .get(&by_path.scope)
                        .is_some_and(|seen| *seen >= WARM_PER_GRAMMAR)
                })
            {
                continue;
            }

            // **Armed as soon as any grammar has a name, and fired on the way
            // out however this turn ends.** See [`Attempt`]: six explicit calls
            // sat on the `continue`s below, and a seventh way out added later
            // would have silently not marked.
            //
            // `None` while the extension has told us nothing, because there is
            // no scope to mark yet and no honest one to guess. It is retargeted
            // below the moment the first line settles the question, and a path
            // that reaches neither resolution is one the frame path also
            // resolves to nothing, so it is never demanded and never needs a
            // mark.
            let mut attempt = Attempt::new(attempted, by_path.map(|by_path| by_path.scope));

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
            //
            // **It is stat-then-act, and the window is real**, which the
            // adversarial review named and which is not closed here. A path
            // component swapped for a symlink between this call and the open
            // below reads wherever it points, because there is no portable way
            // to ask an open handle for its own path. What it reaches is parsed
            // to compile regex patterns and is never drawn, stored or reported,
            // so what leaks through the window is the timing of a parse. Stated
            // rather than defended against, because the honest options are a
            // platform-specific handle API or nothing.
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
            // Counted here, at the read itself, so the number means what
            // [`WarmReport::read`] says it means: files this thread opened
            // and pulled bytes from, whatever happened afterwards.
            report.read += 1;
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

            // Now the text is in hand, so this is the grammar the frame
            // path would resolve and the one `warm` is about to compile.
            // Keyed on the grammar's `Scope`, which is what `syntect`
            // itself treats as a syntax's identity and is a `Copy`
            // bit-packed atom; the `name` is a display string, so keying
            // on it would allocate per path.
            let Some(grammar) = syntax_for(syntaxes, &path, text.lines().next()) else {
                continue;
            };
            let seen = per_grammar.entry(grammar.scope).or_insert(0);
            if *seen >= WARM_PER_GRAMMAR {
                continue;
            }

            // **The grammar that was actually compiled, which is not always the
            // one the path named.** A Qt `.ts` translation file resolves to
            // TypeScript by extension and compiles XML, so the guard is
            // retargeted here rather than left holding the pre-read answer.
            //
            // It still fires on **drop**, after `warm` below, and that ordering
            // is the point: marking on the way in would let a frame on the other
            // thread see the grammar as attempted and parse it while this one is
            // still mid-compile, which is the cliff back on the frame and the
            // whole thing this is for.
            attempt.retarget(grammar.scope);

            warm(syntaxes, &path, text);
            *seen += 1;
            report.warmed += 1;
        }
        report
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
    ///
    /// Clears [`Self::wanted`] as well, so a demand describes the frame in
    /// progress. A caller that acted on the previous frame's list would be
    /// warming for a screen the reader has already scrolled off.
    pub fn pass(&mut self) -> Pass<'_> {
        for entry in &mut self.entries {
            entry.live = false;
        }
        self.wanted.clear();
        self.demanded.clear();
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

    fn spans(
        &mut self,
        path: &str,
        ordinal: usize,
        hunk: &Hunk,
        index: usize,
        first_line: Option<&str>,
    ) -> &[Span] {
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
            attempted,
            wanted,
            demanded,
            ..
        } = self;
        let attempted = attempted.as_deref();

        // `built` is set by the two arms that construct or re-construct a parse,
        // so the counter below is written once instead of in both of them.
        let mut built = false;
        let slot = match found {
            // Already claimed this frame, so its content has been checked
            // already. This is what keeps the digest to once per hunk per frame
            // rather than once per line.
            Some(slot) if entries[slot].live => slot,
            Some(slot) => {
                let content = content_of(hunk);
                // **A deferred entry is rebuilt on the grammar rather than on
                // the content**, and that is the whole of how colour arrives.
                // Its digest still matches, so the arm below would call this a
                // reuse and hand back the plain spans for the rest of the
                // session; what changed is not the hunk but the world outside
                // it, one warm ago.
                let thaw = entries[slot]
                    .deferred()
                    .is_some_and(|scope| compiled(scope, attempted));
                if entries[slot].digest == content.digest && !thaw {
                    // **Neither parsed nor reused when the entry is deferred**,
                    // for the reason the `parsed` counter is guarded the same
                    // way below: a deferred hunk has no `Sides`, so no parse
                    // survived from an earlier frame and saying one did would
                    // overclaim exactly what `HighlightStats::reused` promises.
                    if entries[slot].deferred().is_none() {
                        stats.reused += 1;
                    }
                    entries[slot].live = true;
                    slot
                } else {
                    // **A deferred entry always starts over, whether or not its
                    // grammar has arrived.** It has no parse to rewind into,
                    // because it never made one: `Parse::Deferred` holds no
                    // `Sides`, so `checkpoints` is empty and `rewind` could only
                    // ever return false. Said here rather than left to be
                    // rediscovered from two files away. `thaw` is not a third
                    // disjunct, because it already implies this one.
                    if entries[slot].deferred().is_some() || !entries[slot].rewind(content.clone())
                    {
                        entries[slot] =
                            Entry::new(path, ordinal, content, syntaxes, first_line, attempted);
                    }
                    entries[slot].live = true;
                    built = true;
                    slot
                }
            }
            None => {
                entries.push(Entry::new(
                    path,
                    ordinal,
                    content_of(hunk),
                    syntaxes,
                    first_line,
                    attempted,
                ));
                built = true;
                entries.len() - 1
            }
        };

        // **Counted once, and only when something was actually parsed.** A
        // deferred hunk is a hunk this frame declined to parse, and reporting it
        // as parsed would put a number I2b's gates read out by exactly the hunks
        // the deferral saved. One place rather than the two arms above, which
        // had the same three lines twice and so had two chances to disagree.
        if built && entries[slot].deferred().is_none() {
            stats.parsed += 1;
        }

        // **The demand, renewed every frame it is still true.** Not raised once
        // and remembered: the caller may ignore it, the warmer may fail, and the
        // screen may move, so the honest list is the one this frame would draw
        // in colour if it could.
        if let Some(scope) = entries[slot].deferred()
            && demanded.insert(scope)
        {
            wanted.push(path.to_owned());
        }

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
    /// `first_line` is the file's own line one — [`FileDiff::first_line`]
    /// (crate::FileDiff::first_line) — and feeds the two resolution steps that
    /// need content (`SPEC.md` §6): the shebang fallback and the `.ts` XML
    /// sniff. It is consulted only when an entry is created, so a first line
    /// that changes on its own keeps the cached grammar until the hunk's
    /// content forces a fresh entry (a rewind keeps the old grammar too, and
    /// stays grammar-uniform) — self-healing rather than tracked, because
    /// tracking it would re-key the cache on a fact almost no frame moves.
    ///
    /// # Panics
    ///
    /// If `index` is past the end of `hunk.lines`, the same way indexing a slice
    /// does, and for the same reason [`Frame::diff`](crate::Frame::diff) panics
    /// on a stale index: a caller has to be walking a hunk it holds, and a
    /// lenient accessor would turn that bug into a silently uncoloured row.
    pub fn spans(
        &mut self,
        path: &str,
        ordinal: usize,
        hunk: &Hunk,
        index: usize,
        first_line: Option<&str>,
    ) -> &[Span] {
        self.highlighter
            .spans(path, ordinal, hunk, index, first_line)
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
    /// Hand written because the bundled grammars are a couple of hundred
    /// syntaxes of compiled regex, and a derived `Debug` would put all of them
    /// in whatever this is nested inside.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Highlighter")
            .field("tracked", &self.entries.len())
            .field("stats", &self.stats)
            .finish_non_exhaustive()
    }
}

/// One path's turn at the warmer, which marks its grammar as attempted however
/// that turn ends.
///
/// **A guard rather than a call at every exit, and this module has already paid
/// for learning the difference.** [`Pass`] is the same shape one type over, and
/// its docblock records why: the sweep began as a call the caller had to bracket
/// by hand, and *"two of the five call sites in that same commit had already
/// forgotten it"*. Here there were six `continue`s between a grammar being named
/// and a file being parsed, each carrying the same two lines, and a seventh
/// added later would have compiled, passed, and marked nothing.
///
/// **What forgetting costs is a livelock, not a missed optimisation.** The frame
/// path draws a hunk out of the *diff*, which outlives the file it came from, so
/// a path that vanished between the frame asking and this thread opening it can
/// never be compiled from. Unmarked, it is demanded again on the next frame, and
/// every demand spawns a thread and every thread sends a wake. Marked, such a
/// grammar is parsed cold **once**, which is exactly what shipped before any of
/// this existed.
///
/// Firing on **drop** is also what keeps the ordering right on the way through:
/// a mark written on the way *in* would let a frame on the other thread see the
/// grammar as attempted and parse it while this thread is still mid-compile,
/// which is the cliff back on the frame.
///
/// Poisoning is unreachable: the workspace builds with `panic = "abort"`, so no
/// thread unwinds while holding this lock. Unwrapped through rather than
/// expected, because the set only ever grows and a lock that somehow failed is
/// not worth taking the monitor down over.
struct Attempt<'a> {
    attempted: Option<&'a Mutex<HashSet<Scope>>>,
    /// `None` while no grammar has a name yet, which is an extensionless path
    /// before its first line has been read. Marking nothing is right there: the
    /// frame path resolves such a file the same way and, finding nothing either,
    /// never asks for it.
    scope: Option<Scope>,
}

impl<'a> Attempt<'a> {
    fn new(attempted: Option<&'a Mutex<HashSet<Scope>>>, scope: Option<Scope>) -> Self {
        Self { attempted, scope }
    }

    /// Name the grammar this turn is really about, once the file's first line
    /// has settled it.
    ///
    /// **Two paths reach here and they fail differently without it.** A Qt `.ts`
    /// translation file resolves to TypeScript by extension and compiles XML, so
    /// leaving the pre-read answer in place would mark a grammar nothing
    /// compiled and leave the one that *was* compiled unmarked. An extensionless
    /// script resolves to nothing at all until this is called.
    fn retarget(&mut self, scope: Scope) {
        self.scope = Some(scope);
    }
}

impl Drop for Attempt<'_> {
    fn drop(&mut self) {
        if let (Some(attempted), Some(scope)) = (self.attempted, self.scope) {
            attempted
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(scope);
        }
    }
}

/// Files to warm from the repository's own population: the grammars it leads
/// with, merged across every extension that spells them.
///
/// **The merge is the whole of this function, and it is why the tally comes back
/// unranked.** [`crate::worktree::indexed_extensions`] cannot know that `.yml`
/// and `.yaml` are one grammar, or `.h` and `.hpp`, or `.md` and `.markdown`,
/// because `SPEC.md` §6 keeps `syntect` out of that file. Ranking there would
/// count a language once per spelling at exactly the point where the counts
/// decide which three grammars get compiled, so a repository whose YAML is split
/// evenly across two extensions loses to a smaller single-extension language.
/// Here the grammar is knowable, so the counts are summed on it and the ranking
/// is exact.
///
/// **Resolved from a synthesised `a.<extension>` rather than from a real path**,
/// which is deliberate and narrower than it looks: the group being merged *is*
/// an extension, so the extension rule is the one that has to decide it.
/// Resolving a real path would let `SPEC.md` §6's whole-filename step answer for
/// the group, so one `CMakeLists.txt` would make every `.txt` in the repository
/// warm CMake.
///
/// Ties keep the tally's order, because [`slice::sort_by`] is stable and
/// `indexed_extensions` returns a total order. So two grammars with the same
/// file count are warmed in the same sequence every run, and a gate can assert
/// which three were chosen.
///
/// Bounded at [`WARM_LEADING`] grammars and [`WARM_PER_GRAMMAR`] files each,
/// which is nine against [`WARM_TOTAL`]'s twelve, so `warm_run`'s own caps are
/// the ceiling rather than the plan.
fn leading_paths(syntaxes: &SyntaxSet, root: &Path) -> Vec<String> {
    let mut merged: Vec<(Scope, usize, Vec<String>)> = Vec::new();
    for indexed in crate::worktree::indexed_extensions(root, WARM_PER_GRAMMAR) {
        let probe = format!("a.{}", indexed.extension);
        let Some(syntax) = syntax_for(syntaxes, &probe, None) else {
            continue;
        };
        match merged.iter_mut().find(|(scope, ..)| *scope == syntax.scope) {
            Some((_, files, paths)) => {
                *files += indexed.files;
                let room = WARM_PER_GRAMMAR.saturating_sub(paths.len());
                paths.extend(indexed.paths.into_iter().take(room));
            }
            None => merged.push((syntax.scope, indexed.files, indexed.paths)),
        }
    }
    merged.sort_by(|left, right| right.1.cmp(&left.1));
    merged
        .into_iter()
        .take(WARM_LEADING)
        .flat_map(|(_, _, paths)| paths)
        .collect()
}

/// What a warm calls when it ends, so a caller blocked on something else can
/// find out.
///
/// **A callback rather than a `Sender`, and the reason is the same one that
/// keeps `syntect` out of the shell's vocabulary.** The one caller that wants
/// this is a `ratatui` event loop whose channel carries *its* wake type; a
/// `Sender<()>` here would make it run a bridge thread whose whole job is to
/// turn one message into another. `SPEC.md` §6 asks the core to produce frames
/// and know nothing about the terminal, and a closure is how it says *this
/// happened* without naming who is listening.
///
/// Called exactly once, at the end of the run, however the run ended.
pub type Warmed = Box<dyn FnOnce() + Send + 'static>;

/// Whether the warmer has run over `scope`, so a parse under it will not pay
/// the compile cliff.
///
/// **The question is asked in the negative and that is the point.** A `false`
/// here is exact: nothing in this crate has built a `ParseState` for that
/// grammar, so every pattern it owns is an uncompiled `OnceCell`. A `true` is
/// the weaker half and nothing acts on it beyond declining to defer, which is
/// where this shipped before [#129](https://github.com/breferrari/vigia/issues/129).
/// See [`Highlighter::attempted`].
///
/// `true` for an eager highlighter, which has no set and never defers.
fn compiled(scope: Scope, attempted: Option<&Mutex<HashSet<Scope>>>) -> bool {
    match attempted {
        None => true,
        Some(attempted) => attempted
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains(&scope),
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

/// Grammars [`Highlighter::warm_repository`] will compile from the repository's
/// own population, whatever else it holds.
///
/// **The bound the memory decides**, and it is the one number in this module
/// that is not about time. Measured over this repository, release, warming one
/// grammar at a time and reading RSS after each: baseline **6.73 MiB**, and ten
/// grammars later **64.73 MiB**. Rust alone is +12.43 MiB and Markdown +19 to
/// +35 MiB, because Markdown's grammar embeds most of the others. I3's budget is
/// drift rather than a plateau, so none of that is a breach; it is simply a bad
/// trade, since a monitor left open for days would be carrying the compiled form
/// of languages the session never opens.
///
/// Three, because what a repository *leads* with is barely speculative: the
/// agent in the other pane is near-certain to write the language the repository
/// is mostly made of, so those megabytes are spent within seconds either way and
/// this only moves them earlier. It is the tail that is speculative, and the
/// tail is what the cap removes.
pub const WARM_LEADING: usize = 3;

/// What one [`Highlighter::warm_ahead`] run did.
///
/// Two numbers rather than one, because the thread has two costs and the cap
/// exists to bound both. `warmed` is what it parsed, which the per-grammar cap
/// has always been asserted on. `read` is what it opened, and it went ungated
/// until a change moved the cap's check after the read: the parse count was
/// identical either way, so a run that read sixty-four files to warm three
/// looked exactly like one that read three. A number nothing asserts is a
/// property nothing protects.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WarmReport {
    /// Files parsed, and so grammars actually compiled against.
    pub warmed: usize,
    /// Files opened and read, which the per-grammar cap is meant to keep near
    /// `warmed` rather than near the changed set.
    pub read: usize,
}

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
/// This function therefore makes no such claim, and callers must not build one
/// on top of it.
///
/// **The table above is whole-file parses, and reading it as a frame cost is
/// what made [#51](https://github.com/breferrari/vigia/issues/51) decline more
/// than it had to.** A frame parses one *screenful*, so those numbers carry a
/// large parse alongside the compile they were meant to isolate. Measured again
/// at frame scale, twenty-four lines, fresh `SyntaxSet` per case:
///
/// | | cold | after a real 64KB sibling | floor |
/// |---|---|---|---|
/// | `.rs` | 123.98ms | **2.40ms** | 2.40ms |
/// | `.md` | 694.75ms | 89.77ms | 90.65ms |
/// | `.toml` | 15.26ms | 0.43ms | 0.40ms |
///
/// So the sentence above stays true and stops being the whole story: a *warmth*
/// claim is still unavailable, and a **coldness** claim is exact and is worth
/// having. [`Highlighter::attempted`] is the record this run keeps, it says only
/// that this function has been over a file of that grammar, and the frame path
/// acts on its absence rather than on its presence.
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
    // The text is in hand, so resolution here sees the same first line the
    // frame path will. Note the shebang fallback is unreachable through
    // `warm_ahead`, whose per-grammar cap check runs before the read and so
    // skips an extensionless path; what this buys today is the `.ts` sniff
    // agreeing with the frame path on which grammar a Qt file compiles.
    let Some(syntax) = syntax_for(syntaxes, path, text.lines().next()) else {
        return;
    };
    let mut state = ParseState::new(syntax);
    // `split_inclusive` keeps the trailing newline each line was stored with,
    // which is what the newlines-variant grammars expect; splitting it off
    // would parse every line as though the file ended there.
    for line in text.split_inclusive('\n') {
        // A grammar that fails on a line stops the warm rather than the process.
        // Nothing downstream depends on this having finished, by construction.
        if state.parse_line(line, syntaxes).is_err() {
            return;
        }
    }
}

/// Extensions whose grammar is chosen by a written rule rather than by
/// registration accident, because more than one grammar in the dump claims
/// them. `SPEC.md` §6 carries the reasons row by row; the short form:
///
/// - `.h` and `.m` go to Objective-C over C/C++ and MATLAB because ObjC's
///   grammar is a C superset (C headers colour fully) and it is the dialect of
///   the reader who filed [#235](https://github.com/breferrari/vigia/issues/235).
///   `bat` rules `.h` the other way (C++, their #877), recorded in §6 so
///   flipping a row is one edit and an informed one.
/// - `.v` goes to V over Verilog because the ruling's whole subject is the
///   modern-language set.
/// - `.sass` goes to Sass by name because Ruby Haml also registers the
///   extension, which is how `.sass` drew as Haml for two phases.
/// - `.jsx` goes to the TSX grammar: TSX is a superset of JSX, and the Babel
///   grammar is excluded from the `fancy`-vetted set.
///
/// A named grammar missing from the dump falls through to ordinary
/// resolution rather than to nothing, so the table can name grammars the dump
/// has not vendored yet without turning those files plain in the meantime.
const AMBIGUOUS: [(&str, &str); 5] = [
    ("h", "Objective-C"),
    ("m", "Objective-C"),
    ("v", "V"),
    ("sass", "Sass"),
    ("jsx", "TypeScriptReact"),
];

/// Formats whose own grammar this stack cannot carry, resolved to the nearest
/// grammar in the dump rather than to nothing. `SPEC.md` §6 records each gap
/// with its reason; the short form:
///
/// - Astro's and Bicep's upstream grammars are ST4 `version: 2` files built on
///   `extends:` inheritance, which `syntect` does not implement — and each
///   extends exactly the grammar its row names, so the approximation is the
///   grammar's own base.
/// - Mojo has no `.sublime-syntax` anywhere, and MDX's only real one carries
///   no licence to vendor under; both languages are supersets of the grammar
///   named.
///
/// Consulted **after** ordinary resolution fails, so the day a real grammar
/// for one of these lands in the dump it wins without this table changing —
/// an approximation must never outrank the real thing.
/// Extensions whose grammar cannot be known from the path alone, because a
/// resolution step reads the file's first line.
///
/// One entry today: `SPEC.md` §6's Qt rule, where a `.ts` opening with an XML
/// declaration is a translation file rather than TypeScript. It is a named
/// constant because two places need the same answer — the sniff in
/// [`syntax_for`] and the warmer's pre-read cap check, which may only trust an
/// extension-only resolution for a path that is *not* in here.
const CONTENT_SENSITIVE: [&str; 1] = ["ts"];

/// Whether `path`'s grammar depends on its content. See [`CONTENT_SENSITIVE`].
fn content_sensitive(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            CONTENT_SENSITIVE
                .iter()
                .any(|candidate| ext.eq_ignore_ascii_case(candidate))
        })
}

const NEAREST: [(&str, &str); 5] = [
    ("astro", "HTML"),
    ("bicep", "JavaScript"),
    ("mdx", "Markdown"),
    ("mojo", "Python"),
    ("🔥", "Python"),
];

/// The grammar for `path`, by `SPEC.md` §6's five steps: the ambiguity rules,
/// the whole file name (with a leading-dot retry), the extension, the
/// nearest-grammar approximations of [`NEAREST`], and the file's first line.
///
/// Whole name **before** extension because it is the more specific claim:
/// `CMakeLists.txt` is registered whole by the CMake grammar, and looking up
/// `txt` first handed it to Plain Text — which is exactly what the old
/// two-step did, so `CMakeLists.txt` drew plain for as long as highlighting
/// has existed. The dot retry is for `.gitignore`-shaped names whose grammar
/// registers the bare word. The first line is last and optional: it is how an
/// extensionless shebang script gets a language at all, and how a `.ts` that
/// is really a Qt translation file (an XML document) escapes the TypeScript
/// grammar. `None` is ordinary rather than an error: an unrecognised file
/// draws exactly as it did before there was highlighting at all, which
/// `SPEC.md` §11.1 rules.
fn syntax_for<'s>(
    syntaxes: &'s SyntaxSet,
    path: &str,
    first_line: Option<&str>,
) -> Option<&'s SyntaxReference> {
    let path = Path::new(path);
    let ext = path.extension().and_then(|ext| ext.to_str());

    if let Some(ext) = ext {
        // The one content-aware ambiguity: a `.ts` whose first line is an XML
        // declaration is a Qt translation file, and TypeScript-colouring an
        // XML document is wrong on every token.
        if CONTENT_SENSITIVE
            .iter()
            .any(|candidate| ext.eq_ignore_ascii_case(candidate))
            && first_line.is_some_and(|line| {
                // A BOM survives `trim_start` (U+FEFF is not whitespace), and
                // a BOM'd Qt file is still a Qt file.
                line.trim_start_matches('\u{feff}')
                    .trim_start()
                    .starts_with("<?xml")
            })
            && let Some(syntax) = syntaxes.find_syntax_by_extension("xml")
        {
            return Some(syntax);
        }
        if let Some(syntax) = ruled(syntaxes, ext, &AMBIGUOUS) {
            return Some(syntax);
        }
    }

    if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
        if let Some(syntax) = syntaxes.find_syntax_by_extension(name) {
            return Some(syntax);
        }
        if let Some(bare) = name.strip_prefix('.')
            && let Some(syntax) = syntaxes.find_syntax_by_extension(bare)
        {
            return Some(syntax);
        }
    }

    if let Some(syntax) = ext.and_then(|ext| syntaxes.find_syntax_by_extension(ext)) {
        return Some(syntax);
    }

    if let Some(ext) = ext
        && let Some(syntax) = ruled(syntaxes, ext, &NEAREST)
    {
        return Some(syntax);
    }

    first_line.and_then(|line| syntaxes.find_syntax_by_first_line(line))
}

/// The grammar a rule table names for `ext`, when the dump holds it.
///
/// Shared by the [`AMBIGUOUS`] and [`NEAREST`] steps, which run at different
/// priorities but resolve identically: first matching row wins, and a named
/// grammar missing from the dump falls through to the caller's next step
/// rather than to nothing.
fn ruled<'s>(
    syntaxes: &'s SyntaxSet,
    ext: &str,
    table: &[(&str, &str)],
) -> Option<&'s SyntaxReference> {
    // Case-insensitive, because syntect's own extension lookup is: a rule
    // that `FOO.H` slips past case-sensitively would hand exactly the files
    // the table exists for back to registration accident.
    table
        .iter()
        .find(|(candidate, _)| ext.eq_ignore_ascii_case(candidate))
        .and_then(|(_, grammar)| syntaxes.find_syntax_by_name(grammar))
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
        let mut highlighter = Highlighter::eager();
        let mut pass = highlighter.pass();
        (0..hunk.lines.len())
            .map(|i| pass.spans(path, 0, hunk, i, None).to_vec())
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
        let highlighter = Highlighter::eager();
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

    /// The markup rows, through a real Markdown parse rather than pushed
    /// stacks, because Markdown drew at 4.5% while being a covered language
    /// and nothing could see it (#235). Each assertion is one of the four
    /// elements the issue names as rendering plain: a heading, bold, a code
    /// span, a link.
    #[test]
    fn markdown_reaches_the_markup_classes() {
        let texts = [
            "# A heading",
            "Some **bold** text and `a code span` here.",
            "[a link](https://example.com) closes it.",
        ];
        let source = hunk(texts.iter().map(|t| line(LineKind::Added, t)).collect());
        let spans = spans_for("README.md", &source);

        assert_eq!(
            class_at(&spans[0], texts[0].find('A').unwrap()),
            Class::Function
        );
        assert_eq!(
            class_at(&spans[1], texts[1].find("bold").unwrap()),
            Class::Keyword
        );
        assert_eq!(
            class_at(&spans[1], texts[1].find("code").unwrap()),
            Class::String
        );
        assert_eq!(
            class_at(&spans[2], texts[2].find("a link").unwrap()),
            Class::Constant
        );
        assert_eq!(
            class_at(&spans[2], texts[2].find("https").unwrap()),
            Class::Constant
        );
        // And a bullet's text must stay plain: `markup.list` is a meta scope
        // over the whole item, which is why it has no row.
        let list = hunk(vec![line(LineKind::Added, "- plain list text")]);
        let list_spans = spans_for("README.md", &list);
        assert_eq!(class_at(&list_spans[0], "- plain ".len()), Class::Plain);
    }

    /// The attribute row, through real HTML and CSS parses: both were plain
    /// before #235, and both reach JSX, Vue and Svelte the moment their
    /// grammars resolve.
    #[test]
    fn an_attribute_name_is_no_longer_plain() {
        let source = hunk(vec![line(LineKind::Added, "<a href=\"x\">t</a>")]);
        let spans = spans_for("index.html", &source);
        assert_eq!(class_at(&spans[0], "<a ".len()), Class::Variable);
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
        let mut highlighter = Highlighter::eager();
        let spans = highlighter
            .pass()
            .spans("a/b.zzzznope", 0, &source, 0, None)
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
        let syntaxes = &Highlighter::eager().syntaxes;
        assert!(syntax_for(syntaxes, "src/lib.rs", None).is_some());
        assert!(syntax_for(syntaxes, "Makefile", None).is_some());
        assert!(syntax_for(syntaxes, "deep/nested/Makefile", None).is_some());
        assert!(syntax_for(syntaxes, "src/no-such-thing.zzzznope", None).is_none());
    }

    /// The grammar `path` resolves to, by name, or what it failed on.
    fn grammar_of(path: &str, first_line: Option<&str>) -> String {
        let highlighter = Highlighter::eager();
        syntax_for(&highlighter.syntaxes, path, first_line)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "<none>".to_owned())
    }

    /// §6's ambiguity table, row by row. Each of these had two grammars (or a
    /// wrong one) claiming the extension, and each answer here is the written
    /// rule rather than whichever grammar happened to register first.
    #[test]
    fn an_ambiguous_extension_resolves_by_rule_not_registration_order() {
        assert_eq!(grammar_of("include/thing.h", None), "Objective-C");
        assert_eq!(grammar_of("Sources/AppDelegate.m", None), "Objective-C");
        // `.sass` resolved to Ruby Haml for two phases; unresolved would beat
        // that, and Sass is in the dump, so the rule finds the real grammar.
        assert_eq!(grammar_of("styles/site.sass", None), "Sass");
        // TSX is a superset of JSX, and the Babel grammar is fancy-excluded.
        assert_eq!(grammar_of("src/App.jsx", None), "TypeScriptReact");
        // An ordinary TypeScript file is TypeScript...
        assert_eq!(grammar_of("src/main.ts", None), "TypeScript");
        // ...and a Qt translation file is an XML document whose extension
        // lies; the first line is what tells them apart.
        assert_eq!(
            grammar_of(
                "i18n/app_de.ts",
                Some("<?xml version=\"1.0\" encoding=\"utf-8\"?>")
            ),
            "XML"
        );
        // A BOM survives trim_start (U+FEFF is not whitespace) and must not
        // defeat the sniff.
        assert_eq!(
            grammar_of("i18n/app_de.ts", Some("\u{feff}<?xml version=\"1.0\"?>")),
            "XML"
        );
    }

    /// The rules are case-insensitive, because syntect's own extension lookup
    /// is: a `FOO.H` that slipped past the table case-sensitively would
    /// resolve by registration accident, which is the exact failure the table
    /// exists to end.
    #[test]
    fn an_uppercase_extension_resolves_by_the_same_rule() {
        assert_eq!(grammar_of("INCLUDE/THING.H", None), "Objective-C");
        assert_eq!(grammar_of("DOCS/POST.MDX", None), "Markdown");
        assert_eq!(
            grammar_of("APP_DE.TS", Some("<?xml version=\"1.0\"?>")),
            "XML"
        );
    }

    /// Step three's leading-dot retry and step four's first-line fallback,
    /// which are what give dotfiles and extensionless scripts a language.
    #[test]
    fn dotfiles_and_shebang_scripts_resolve() {
        assert_eq!(grammar_of(".gitignore", None), "Git Ignore");
        assert_eq!(grammar_of("Dockerfile", None), "Dockerfile");
        assert_eq!(grammar_of("cmake/CMakeLists.txt", None), "CMake");
        assert_eq!(grammar_of("go.mod", None), "Gomod");
        // No extension, no known name: the shebang is all there is.
        assert_eq!(
            grammar_of("scripts/deploy", Some("#!/usr/bin/env bash")),
            "Bourne Again Shell (bash)"
        );
        // And without a first line the same path stays plain, which §11.1
        // rules is ordinary rather than an error.
        assert_eq!(grammar_of("scripts/deploy", None), "<none>");
    }

    /// The nearest-grammar approximations: formats whose own grammar this
    /// stack cannot carry, each resolved to the grammar its upstream builds on
    /// (or the language it is a superset of) rather than to nothing. §6
    /// records the gaps.
    #[test]
    fn a_gap_format_resolves_to_its_nearest_grammar() {
        assert_eq!(grammar_of("src/pages/index.astro", None), "HTML");
        assert_eq!(grammar_of("infra/main.bicep", None), "JavaScript");
        assert_eq!(grammar_of("docs/post.mdx", None), "Markdown");
        assert_eq!(grammar_of("kernels/matmul.mojo", None), "Python");
        assert_eq!(grammar_of("kernels/matmul.🔥", None), "Python");
        // And the ruled formats that DO have their own grammar now: the
        // survey's headline plus the whole vendored tail.
        assert_eq!(grammar_of("cli/main.v", None), "V");
        assert_eq!(grammar_of("src/app.gleam", None), "Gleam");
        assert_eq!(grammar_of("deploy.ps1", None), "PowerShell");
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
