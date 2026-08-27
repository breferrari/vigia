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
//! | Parse a 1006-line hunk whole | 60.97ms, which is 3.8x over I9 |
//! | Hash that hunk to revalidate it | 7.1µs |
//!
//! The third row is the one the design is built around, and it is the same
//! finding I2a made about re-diffing: the obvious implementation breaks the
//! frame budget on its own rather than merely wasting work. Under I9's own
//! shape, one line rewritten before every frame, parsing a hunk whole would
//! re-parse a thousand lines every frame. So a hunk is parsed forward only as
//! far as something has asked for, and what it has parsed is kept.
//!
//! Three decisions follow, and each is a budget rather than a preference.
//!
//! A hunk is identified by its content, never by a generation counter. A
//! counter bumped whenever the frame path recomputes a diff would be free to
//! check and wrong: inside the two-second settle margin the frame path
//! legitimately re-diffs an untouched file on every frame, so every hunk of it
//! would look new and I2b would fail on files nobody edited. Hashing 44 KiB
//! costs 7.1µs, is exact where it matters, and its one failure mode is a
//! sixty-four-bit collision showing stale colour rather than stale content.
//!
//! Two parse states per hunk, one for each side of the diff. Context feeds
//! both, a removal feeds the index side and an addition feeds the working-tree
//! side. Running the display order through a single state instead is cheaper and
//! visibly wrong: removing a line that opens a string and adding one that opens
//! it again applies the construct twice, and the rest of the hunk turns into
//! string. The cost is that a context line is parsed twice, which is at most six
//! lines per change group and none at all in an all-additions hunk.
//!
//! The cache is bounded by the viewport, not by the diff and not by the
//! session. [`Highlighter::pass`] hands out a guard that drops everything the
//! frame did not draw, and it sweeps in `Drop` rather than asking a caller to,
//! so a bulk edit across ten thousand files cannot grow it: the screen is the
//! bound. That is a stronger claim than the frame path's own, which is bounded
//! by the current diff, and it is what keeps I3 out of reach of this module. It
//! costs a re-parse when a hunk scrolls off and back, and a screenful is 1.53ms.
//!
//! A changed hunk rewinds rather than starting over. Throwing the parse away
//! made a frame cost the reader's scroll depth rather than what it drew, on
//! every frame, for as long as the file being read was the file being written:
//! 53ms p99 five hundred rows in. See [`CHECKPOINT_STRIDE`].
//!
//! What this module does not do is decide a colour. It maps a syntax scope
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// Bytes this run covers.
    pub len: usize,
    /// What those bytes mean.
    pub class: Class,
}

/// What a [`Highlighter`] has done since it was created.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HighlightStats {
    /// Hunks parsed from scratch: never seen, or seen with different content.
    pub parsed: u64,
    /// Hunks whose parse survived from an earlier frame.
    pub reused: u64,
    /// Lines actually run through the parser.
    pub lines: u64,
    /// Bytes of those lines.
    pub bytes: u64,
    /// Hunks dropped because they left the viewport.
    pub evicted: u64,
}

/// Scope prefixes and what each one means, most specific first.
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
    // A heading's text: Sublime's Markdown grammar scopes it `entity.name.section`
    // *inside* `markup.heading`, and innermost-first classification reaches it before
    // the markup row ever gets asked, so without this row a heading reads as a type
    // while its `#` reads as a heading.
    ("entity.name.section", Class::Function),
    ("entity.name", Class::Type),
    // HTML attributes, CSS selectors, and their JSX/Vue/Svelte descendants.
    ("entity.other.attribute-name", Class::Variable),
    ("support.function", Class::Function),
    ("support.type", Class::Type),
    ("support.class", Class::Type),
    ("variable.language", Class::Keyword),
    ("variable", Class::Variable),
    ("markup.heading", Class::Function),
    ("markup.bold", Class::Keyword),
    ("markup.italic", Class::Keyword),
    ("markup.raw", Class::String),
    ("markup.underline.link", Class::Constant),
    ("markup.quote", Class::Comment),
    // The one deliberate `meta.*` row.
    ("meta.link.inline.description", Class::Constant),
];

/// Lines between the parse positions a later frame can rewind to.
pub const CHECKPOINT_STRIDE: usize = 32;

/// Hunks kept after they have left the screen.
pub const RETAINED_HUNKS: usize = 4;

/// Both sides of one hunk, each parsed as the file it describes.
#[derive(Clone)]
struct Sides {
    /// The index side: context and removals.
    old: Side,
    /// The working-tree side: context and additions.
    new: Side,
}

/// One file's parse position: where the grammar is, and what scope it is under.
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
        // failing the frame.
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
    fn parse(
        &mut self,
        line: &Line,
        buf: &mut String,
        syntaxes: &SyntaxSet,
        table: &[(Scope, Class)],
    ) -> Vec<Span> {
        // The embedded dump is built from the newlines variant (`xtask` merges onto
        // `two_face::syntax::extra_newlines` and loads extras with
        // lines-include-newline); the no-newline form is documented as unreliable
        // because grammars anchor on end of line.
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
    marks: Vec<u64>,
    /// Parse positions this entry can rewind to, one per whole stride
    /// parsed, deepest last.
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
            // The one branch this whole change turns on.
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
    fn is(&self, path: &str, ordinal: usize) -> bool {
        self.ordinal == ordinal && self.path == path
    }

    /// Keep as much of this parse as `content` still agrees with, and report
    /// whether anything survived.
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
    syntaxes: Arc<SyntaxSet>,
    /// [`CLASSES`] resolved once, because a [`Scope`] is an interned atom and
    /// building one from a string takes a lock on syntect's global repository.
    table: Vec<(Scope, Class)>,
    /// A `Vec` rather than a map, and that is a consequence of the viewport bound
    /// rather than an oversight.
    entries: Vec<Entry>,
    /// Hunks that have left the screen, newest last, capped at
    /// [`RETAINED_HUNKS`].
    retired: VecDeque<Entry>,
    stats: HighlightStats,
    /// Grammars the warmer has run over, shared with every thread it spawns.
    attempted: Option<Arc<Mutex<HashSet<Scope>>>>,
    /// Paths whose grammar was uncompiled during the pass in progress, one per
    /// grammar, in the order the frame reached them.
    wanted: Vec<String>,
    /// Scopes already on `wanted` this pass, so the dedup above costs no scan.
    demanded: HashSet<Scope>,
}

impl Highlighter {
    /// Load the bundled grammars and start with an empty cache.
    pub fn new() -> Self {
        Self {
            attempted: Some(Arc::new(Mutex::new(HashSet::new()))),
            ..Self::eager()
        }
    }

    /// The same highlighter, parsing whatever it resolves, cold or not.
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
    pub fn wanted(&self) -> &[String] {
        &self.wanted
    }

    /// Compile grammars ahead of the reader, on a thread, and report how many
    /// files it managed.
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

    /// Compile the grammars this repository leads with, on a thread.
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
            // The total, which the per-grammar cap does not bound. A
            // polyglot changed set has as many budgets as it has languages:
            // fifty distinct extensions warmed forty-three files in
            // 3.93s of held core before this line existed, against the
            // 1.053s worst case the per-grammar cap was reasoned about with.
            // No single-language fixture can see that, which is `SPEC.md`
            // §7's ASCII-fixture rule one axis over.
            if report.warmed >= WARM_TOTAL {
                break;
            }

            // Repository-relative, and refused otherwise.
            if !std::path::Path::new(&path).components().all(|c| {
                matches!(
                    c,
                    std::path::Component::Normal(_) | std::path::Component::CurDir
                )
            }) {
                continue;
            }

            // Looked up before anything is read, which is what makes the per-grammar
            // cap save the I/O and not merely the parse.
            let by_path = syntax_for(syntaxes, &path, None);

            // The cap, cheaply, on the answer the path alone gives.
            if !content_sensitive(&path)
                && by_path.is_some_and(|by_path| {
                    per_grammar
                        .get(&by_path.scope)
                        .is_some_and(|seen| *seen >= WARM_PER_GRAMMAR)
                })
            {
                continue;
            }

            // Armed as soon as any grammar has a name, and fired on the way out however
            // this turn ends.
            let mut attempt = Attempt::new(attempted, by_path.map(|by_path| by_path.scope));

            // And where it actually lands, which the component check cannot know.
            let Ok(target) = std::fs::canonicalize(root.join(&path)) else {
                continue;
            };
            if !target.starts_with(&canonical_root) {
                continue;
            }

            // Bounded at the read rather than after it: see `WARM_BYTES`.
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
            // A bounded read lands mid-codepoint on any file that is not ASCII, so the
            // tail is trimmed to the last complete character rather than the read being
            // widened to avoid it.
            let text = std::str::from_utf8(&buf)
                .unwrap_or_else(|e| std::str::from_utf8(&buf[..e.valid_up_to()]).unwrap_or(""));

            // Nothing compiled means nothing spent, which is the rule a vanished path
            // already follows.
            if text.is_empty() {
                continue;
            }

            // Now the text is in hand, so this is the grammar the frame path would
            // resolve and the one `warm` is about to compile.
            let Some(grammar) = syntax_for(syntaxes, &path, text.lines().next()) else {
                continue;
            };
            let seen = per_grammar.entry(grammar.scope).or_insert(0);
            if *seen >= WARM_PER_GRAMMAR {
                continue;
            }

            // The grammar that was actually compiled, which is not always the one the
            // path named.
            attempt.retarget(grammar.scope);

            warm(syntaxes, &path, text);
            *seen += 1;
            report.warmed += 1;
        }
        report
    }

    /// Begin a frame, and hand back the only thing that can ask for spans.
    pub fn pass(&mut self) -> Pass<'_> {
        for entry in &mut self.entries {
            entry.live = false;
        }
        self.wanted.clear();
        self.demanded.clear();
        Pass { highlighter: self }
    }

    /// Retire every hunk the pass did not draw, and drop what will not fit.
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

        // An index walk rather than `Vec::extract_if`, which would say this in one
        // line: that is stable since 1.87 and the workspace declares 1.85, and
        // correcting the manifest is a toolchain decision rather than something to slip
        // into a rendering fix.
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
        // One scan, and the miss is where the retired queue is consulted, so a hunk the
        // reader has scrolled back to lands in `entries` and everything below sees one
        // cache rather than two.
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
                // A deferred entry is rebuilt on the grammar rather than on the
                // content, and that is the whole of how colour arrives.
                let thaw = entries[slot]
                    .deferred()
                    .is_some_and(|scope| compiled(scope, attempted));
                if entries[slot].digest == content.digest && !thaw {
                    // Neither parsed nor reused when the entry is deferred,
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
                    // A deferred entry always starts over, whether or not its grammar
                    // has arrived.
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

        // Counted once, and only when something was actually parsed. A deferred hunk is
        // a hunk this frame declined to parse, and reporting it as parsed would put a
        // number I2b's gates read out by exactly the hunks the deferral saved.
        if built && entries[slot].deferred().is_none() {
            stats.parsed += 1;
        }

        // The demand, renewed every frame it is still true.
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
    pub fn tracked(&self) -> usize {
        self.entries.len() + self.retired.len()
    }
}

/// One frame's worth of highlighting, which sweeps the cache when it is dropped.
pub struct Pass<'h> {
    highlighter: &'h mut Highlighter,
}

impl Pass<'_> {
    /// Spans for display line `index` of `hunk`, which is hunk `ordinal` of
    /// `path`.
    /// # Panics
    ///
    /// If `index` is past the end of `hunk.lines`, the same way indexing a slice
    /// does.
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
struct Attempt<'a> {
    attempted: Option<&'a Mutex<HashSet<Scope>>>,
    /// `None` while no grammar has a name yet, which is an extensionless path before
    /// its first line has been read.
    scope: Option<Scope>,
}

impl<'a> Attempt<'a> {
    fn new(attempted: Option<&'a Mutex<HashSet<Scope>>>, scope: Option<Scope>) -> Self {
        Self { attempted, scope }
    }

    /// Name the grammar this turn is really about, once the file's first line
    /// has settled it.
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
    merged.sort_by_key(|left| std::cmp::Reverse(left.1));
    merged
        .into_iter()
        .take(WARM_LEADING)
        .flat_map(|(_, _, paths)| paths)
        .collect()
}

/// What a warm calls when it ends, so a caller blocked on something else can
/// find out.
pub type Warmed = Box<dyn FnOnce() + Send + 'static>;

/// Whether the warmer has run over `scope`, so a parse under it will not pay
/// the compile cliff.
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
pub const WARM_FILES: usize = 64;

/// Files [`Highlighter::warm_ahead`] will parse per grammar.
pub const WARM_PER_GRAMMAR: usize = 3;

/// Files [`Highlighter::warm_ahead`] will parse in total, whatever the mix.
pub const WARM_TOTAL: usize = 12;

/// Bytes of each file [`Highlighter::warm_ahead`] will read and parse, at most.
pub const WARM_BYTES: usize = 64 * 1024;

/// Grammars [`Highlighter::warm_repository`] will compile from the repository's
/// own population, whatever else it holds.
pub const WARM_LEADING: usize = 3;

/// What one [`Highlighter::warm_ahead`] run did.
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
/// A free function over `&SyntaxSet` rather than a method, because the caller is on
/// another thread and the compiled patterns land in the set both of them share.
///
/// There is no such thing as a warm grammar: `fancy_regex` compiles per *pattern*,
/// so warming on one file leaves a different file of the same language still
/// paying. This records nothing and claims nothing, and callers must not build a
/// `warmed: HashSet<Grammar>` on top of it — that would report a grammar warm and
/// let the next file pay tens of milliseconds against I9's 16ms.
/// [`Highlighter::attempted`] is the record, it says only that this function has
/// been over a file of that grammar, and the frame path acts on its absence rather
/// than on its presence.
///
/// A path with no grammar is a no-op. Crate-private, which is what keeps `syntect`
/// out of `vigia`'s vocabulary (`SPEC.md` §6); [`Highlighter::warm_ahead`] is the
/// way in.
fn warm(syntaxes: &SyntaxSet, path: &str, text: &str) {
    // The text is in hand, so resolution here sees the same first line the frame path
    // will.
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

    use super::*;

    fn line(kind: LineKind, text: &str) -> Line {
        Line {
            kind,
            text: text.to_owned(),
            emph: Vec::new(),
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

    /// The markup rows, through a real Markdown parse rather than pushed stacks,
    /// because Markdown drew at 4.5% while being a covered language and nothing could
    /// see it .
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

    /// The attribute row, through real HTML and CSS parses: both were plain before ,
    /// and both reach JSX, Vue and Svelte the moment their grammars resolve.
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
