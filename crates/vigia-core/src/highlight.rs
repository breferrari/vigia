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
//! session. [`Highlighter::begin`] and [`Highlighter::sweep`] bracket a frame and
//! drop everything it did not draw, so a bulk edit across ten thousand files
//! cannot grow it: the screen is the bound. That is a stronger claim than the
//! frame path's own, which is bounded by the current diff, and it is what keeps
//! I3 out of reach of this module. It costs a re-parse when a hunk scrolls off
//! and back, and a screenful is 1.53ms.
//!
//! What this module does **not** do is decide a colour. It maps a syntax scope
//! onto one of nine [`Class`]es and stops, because `SPEC.md` §6 puts no terminal
//! in this crate and §11.1 leaves the palette to the shell.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};

use crate::hunk::{Hunk, Line, LineKind};

/// What a run of characters means, with no colour attached.
///
/// Nine, and the five the README mockup names are load bearing rather than a
/// starting point: `assets/preview.svg` is published, so which distinctions are
/// worth a colour was decided before this code existed (`SPEC.md` §5.1). String,
/// number and comment are added because a diff of real source is unreadable
/// without them and the mockup's sample lines happen not to contain one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

/// Both sides of one hunk, each parsed as the file it describes.
///
/// `Clone` because a [`Checkpoint`] is one of these frozen at a line boundary.
/// The newline scratch buffer is deliberately **not** in here: it lives on
/// [`Entry`], so that cloning a parse position does not clone a buffer whose
/// contents are worthless one line later.
#[derive(Clone)]
struct Sides {
    /// The index side: context and removals.
    old: ParseState,
    old_stack: ScopeStack,
    /// The working-tree side: context and additions.
    new: ParseState,
    new_stack: ScopeStack,
}

impl Sides {
    fn new(syntax: &SyntaxReference) -> Self {
        Self {
            old: ParseState::new(syntax),
            old_stack: ScopeStack::new(),
            new: ParseState::new(syntax),
            new_stack: ScopeStack::new(),
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
            LineKind::Removed => spans_of(
                &mut self.old,
                &mut self.old_stack,
                buf,
                line,
                syntaxes,
                table,
            ),
            LineKind::Added => spans_of(
                &mut self.new,
                &mut self.new_stack,
                buf,
                line,
                syntaxes,
                table,
            ),
            LineKind::Context => {
                advance(&mut self.old, &mut self.old_stack, buf, syntaxes);
                spans_of(
                    &mut self.new,
                    &mut self.new_stack,
                    buf,
                    line,
                    syntaxes,
                    table,
                )
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
/// Created once and driven per frame: [`begin`](Highlighter::begin), then
/// [`spans`](Highlighter::spans) for every line drawn, then
/// [`sweep`](Highlighter::sweep).
///
/// ```no_run
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let worktree = vigia_core::Worktree::discover(".")?;
/// let mut frame = worktree.frame();
/// let mut highlighter = vigia_core::Highlighter::new();
/// frame.advance()?;
///
/// highlighter.begin();
/// let (_, diff) = frame.diff(0)?;
/// let path = diff.path.clone();
/// let hunk = diff.hunks[0].clone();
/// for i in 0..hunk.lines.len() {
///     println!("{:?}", highlighter.spans(&path, 0, &hunk, i));
/// }
/// highlighter.sweep();
/// # Ok(())
/// # }
/// ```
pub struct Highlighter {
    syntaxes: SyntaxSet,
    /// [`CLASSES`] resolved once, because a [`Scope`] is an interned atom and
    /// building one from a string takes a lock on syntect's global repository.
    table: Vec<(Scope, Class)>,
    /// A `Vec` rather than a map, and that is a consequence of the viewport
    /// bound rather than an oversight. Sweeping every frame keeps this to the
    /// hunks one screen can show, which is a few dozen at the very most, so a
    /// linear scan is faster than hashing a key and is one less thing to get
    /// wrong.
    entries: Vec<Entry>,
    stats: HighlightStats,
}

impl Highlighter {
    /// Load the bundled grammars and start with an empty cache.
    ///
    /// Costs 318µs measured in release, which is why it is done up front rather
    /// than behind a lazy initialiser: I7 gives startup 50ms, and hiding this
    /// behind first use would only move it onto the first frame that draws.
    pub fn new() -> Self {
        Self {
            syntaxes: SyntaxSet::load_defaults_newlines(),
            table: CLASSES
                .iter()
                .filter_map(|(prefix, class)| Some((Scope::new(prefix).ok()?, *class)))
                .collect(),
            entries: Vec::new(),
            stats: HighlightStats::default(),
        }
    }

    /// Open a frame. Everything not asked for before [`Highlighter::sweep`] is
    /// dropped.
    pub fn begin(&mut self) {
        for entry in &mut self.entries {
            entry.live = false;
        }
    }

    /// Drop every hunk this frame did not draw.
    ///
    /// The whole of the I3 claim. Without it the cache would grow with
    /// everything ever scrolled past, which over a runtime measured in days is
    /// the same thing as unbounded.
    pub fn sweep(&mut self) {
        let before = self.entries.len();
        self.entries.retain(|entry| entry.live);
        self.stats.evicted += (before - self.entries.len()) as u64;
    }

    /// Spans for display line `index` of `hunk`, which is hunk `ordinal` of
    /// `path`.
    ///
    /// Reuses the previous frame's parse when the hunk's content is unchanged,
    /// and parses forward within it when it is not. Content, not identity: a
    /// diff recomputed inside the settle margin holds the same hunks, and
    /// treating those as new would re-highlight files nobody edited.
    ///
    /// # Panics
    ///
    /// If `index` is past the end of `hunk.lines`, the same way indexing a slice
    /// does, and for the same reason [`Frame::diff`](crate::Frame::diff) panics
    /// on a stale index: a caller has to be walking a hunk it holds, and a
    /// lenient accessor would turn that bug into a silently uncoloured row.
    pub fn spans(&mut self, path: &str, ordinal: usize, hunk: &Hunk, index: usize) -> &[Span] {
        // Destructured so the syntax set can be read while one entry and the
        // counters are written. Through `&mut self` alone the borrow checker
        // sees one whole thing.
        let Self {
            syntaxes,
            table,
            entries,
            stats,
        } = self;

        let found = entries
            .iter()
            .position(|entry| entry.ordinal == ordinal && entry.path == path);
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
    /// At most what one screen can show. I3 is a claim about a process that runs
    /// for days, so this is the number that says the cache is bounded by the
    /// viewport rather than by the session.
    pub fn tracked(&self) -> usize {
        self.entries.len()
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

/// Run a line through a side without building spans for it.
///
/// What a context line costs the side it is not drawn from.
fn advance(state: &mut ParseState, stack: &mut ScopeStack, buf: &str, syntaxes: &SyntaxSet) {
    let Ok(ops) = state.parse_line(buf, syntaxes) else {
        return;
    };
    for (_, op) in &ops {
        if stack.apply(op).is_err() {
            return;
        }
    }
}

/// Run a line through a side and turn its scope changes into spans.
fn spans_of(
    state: &mut ParseState,
    stack: &mut ScopeStack,
    buf: &str,
    line: &Line,
    syntaxes: &SyntaxSet,
    table: &[(Scope, Class)],
) -> Vec<Span> {
    let text_len = line.text.len();
    // A grammar that fails on a line leaves that line uncoloured rather than
    // failing the frame. A monitor survives the file it cannot read; `SPEC.md`
    // §2 makes a runtime measured in days turn every transient failure into a
    // certainty.
    let Ok(ops) = state.parse_line(buf, syntaxes) else {
        return plain(text_len);
    };

    let mut spans: Vec<Span> = Vec::new();
    let mut at = 0usize;
    for (offset, op) in &ops {
        // Clamped, because `buf` carries the newline the grammars need and the
        // line does not. Everything at or past the end contributes no span while
        // still advancing the state the next line starts from.
        let offset = (*offset).min(text_len);
        if offset > at {
            push(&mut spans, offset - at, classify(stack, table));
            at = offset;
        }
        if stack.apply(op).is_err() {
            break;
        }
    }
    push(&mut spans, text_len - at, classify(stack, table));
    spans
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
        highlighter.begin();
        (0..hunk.lines.len())
            .map(|i| highlighter.spans(path, 0, hunk, i).to_vec())
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
        highlighter.begin();
        let spans = highlighter.spans("a/b.zzzznope", 0, &source, 0).to_vec();

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
