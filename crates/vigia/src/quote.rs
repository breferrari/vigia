//! What of an agent's answer is code, and how a quoted line is drawn.
//!
//! An answer is free text an agent wrote with no schema, so the only code found
//! here is code the agent declared: a fenced block, and a run between backticks.
//! An indented block is words, that being the shape a continued list item also
//! has, and colouring prose as code reads worse than colouring nothing. Code
//! breaks at the column and never at the last blank, which is right for a
//! sentence and cuts a statement where nobody would.

use std::borrow::Cow;
use std::ops::Range;

use vigia_core::{Class, Span};

/// A run of a note row's text and the ink it takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run {
    /// Bytes this run covers.
    pub len: usize,
    /// `None` is the row's own voice; `Some` is quoted code, in the class a
    /// grammar gave it or [`Class::Plain`] where none did.
    pub class: Option<Class>,
}

/// One stretch of an answer: what the agent wrote as words, or what it fenced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Chunk {
    /// Words, newlines and all, to be broken the way prose is.
    Prose(String),
    /// A fenced block, its fences dropped.
    Code {
        /// What the opening fence called the language, where it called it one.
        token: Option<String>,
        /// The block's lines, in order.
        lines: Vec<String>,
    },
}

/// The fewest backticks that open a fence.
const FENCE: usize = 3;

/// How many backticks a fence opens with and what it names after them, or
/// `None` where `line` is not one.
fn fence(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim();
    let named = trimmed.trim_start_matches('`');
    let ticks = trimmed.len() - named.len();
    // A name holding a backtick names nothing, which is what keeps a fence that
    // opens and closes on one line from asking the dump for a language spelled
    // with the fence that closed it, and drawing plain for the want of one.
    let named = named.trim();
    let named = if named.contains('`') { "" } else { named };
    (ticks >= FENCE).then_some((ticks, named))
}

/// `text`'s lines, each without the carriage return a store round-trip keeps.
///
/// An agent writes over a pipe rather than into a file, so its line endings
/// are its own platform's, and one left on the end draws as the unprintable
/// mark at the end of every line it wrote.
pub fn lines_of(text: &str) -> impl Iterator<Item = &str> {
    text.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
}

/// `reply` split into what the agent wrote as words and what it fenced.
///
/// A fence left open runs to the end. A reply with no fence is one
/// [`Chunk::Prose`] holding it exactly, so an answer that quoted nothing is
/// broken by the rule it always was.
#[must_use]
pub fn chunks(reply: &str) -> Vec<Chunk> {
    let mut out = Vec::new();
    let mut prose: Vec<&str> = Vec::new();
    let mut token: Option<String> = None;
    let mut lines: Vec<String> = Vec::new();
    let mut fenced = false;
    let mut opened = FENCE;

    for line in lines_of(reply) {
        let marker = fence(line);
        // A block closes on a fence at least as long as the one that opened
        // it, which is what lets a block of four quote a block of three whole.
        let closes = marker.is_some_and(|(ticks, _)| ticks >= opened);
        if fenced {
            if closes {
                out.push(Chunk::Code {
                    token: token.take(),
                    lines: std::mem::take(&mut lines),
                });
                fenced = false;
            } else {
                lines.push(line.to_owned());
            }
        } else if let Some((ticks, named)) = marker {
            if !prose.is_empty() {
                out.push(Chunk::Prose(prose.join("\n")));
                prose.clear();
            }
            token = (!named.is_empty()).then(|| named.to_owned());
            opened = ticks;
            fenced = true;
        } else {
            prose.push(line);
        }
    }

    if fenced {
        out.push(Chunk::Code { token, lines });
    } else if !prose.is_empty() {
        out.push(Chunk::Prose(prose.join("\n")));
    }
    out
}

/// `paragraph` with its backticked runs unwrapped, and where they landed.
///
/// A run takes no grammar, a backticked word being as often a path or a flag as
/// it is code, an unclosed backtick is a character the agent wrote, and a
/// paragraph with no pair in it comes back with no runs at all.
#[must_use]
pub fn inline(paragraph: &str) -> (Cow<'_, str>, Vec<Run>) {
    let mut out = String::new();
    let mut runs: Vec<Run> = Vec::new();
    let mut rest = paragraph;
    while let Some(open) = rest.find('`') {
        let Some(close) = rest[open + 1..].find('`') else {
            break;
        };
        let close = open + 1 + close;
        push(&mut runs, open, None);
        out.push_str(&rest[..open]);
        push(&mut runs, close - open - 1, Some(Class::Plain));
        out.push_str(&rest[open + 1..close]);
        rest = &rest[close + 1..];
    }
    // No pair, so nothing was unwrapped and the paragraph is its own answer. The
    // empty run list is what the painter reads as "this row is all one voice",
    // which is the cheap path every answer that quoted nothing takes.
    if runs.is_empty() {
        return (Cow::Borrowed(paragraph), Vec::new());
    }
    push(&mut runs, rest.len(), None);
    out.push_str(rest);
    (Cow::Owned(out), runs)
}

/// A run of bytes within one line, which a wrapped row has to clip and re-base.
///
/// Two vocabularies for one walk: [`Span`] is what a grammar said about a line of
/// the diff, and [`Run`] is that plus the answer's own voice, which no `Span` can
/// say. The walk itself is the same either way.
pub trait Sliced: Copy {
    /// Bytes this run covers.
    fn bytes(self) -> usize;
    /// The same run over `bytes` bytes instead.
    fn resized(self, bytes: usize) -> Self;
}

impl Sliced for Run {
    fn bytes(self) -> usize {
        self.len
    }

    fn resized(self, bytes: usize) -> Self {
        Self { len: bytes, ..self }
    }
}

impl Sliced for Span {
    fn bytes(self) -> usize {
        self.len
    }

    fn resized(self, bytes: usize) -> Self {
        Self { len: bytes, ..self }
    }
}

/// `runs` clipped to the byte range `piece` and re-based onto it.
#[must_use]
pub fn rebase<T: Sliced>(runs: &[T], piece: &Range<usize>) -> Vec<T> {
    let mut out = Vec::with_capacity(runs.len());
    let mut at = 0usize;
    for run in runs {
        let from = at.max(piece.start);
        let to = (at + run.bytes()).min(piece.end);
        if to > from {
            out.push(run.resized(to - from));
        }
        at += run.bytes();
        if at >= piece.end {
            break;
        }
    }
    out
}

/// One display row of a quoted block: its bytes, what each run of them means,
/// and the columns a continuation stands in by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeRow {
    /// This row's piece of the line.
    pub text: String,
    /// What each run of `text` means, covering it exactly.
    pub runs: Vec<Run>,
    /// Columns of leading blank before the text, so nested code keeps its block
    /// shape. Zero on the row a line starts on.
    pub indent: usize,
}

/// A quoted block's lines as rows of at most `room` columns, with `spans` from
/// the grammar covering each line.
#[must_use]
pub fn code_rows(lines: &[String], spans: &[Vec<Span>], room: usize) -> Vec<CodeRow> {
    let mut out = Vec::new();
    for (at, line) in lines.iter().enumerate() {
        let runs = runs_of(spans.get(at).map_or(&[][..], Vec::as_slice), line.len());
        // No room to break into, so the row is the line and the painter clips it.
        if room == 0 {
            out.push(CodeRow {
                text: line.clone(),
                runs,
                indent: 0,
            });
            continue;
        }
        // There is never a cut per byte, so the line's own length bounds the walk
        // without inventing a number for it.
        let cuts = crate::render::breaks_of(line, room, line.len());
        let indent = crate::render::indent_of(line, room);
        let mut start = 0usize;
        for cut in cuts.iter().copied().chain(std::iter::once(line.len())) {
            out.push(CodeRow {
                text: line[start..cut].to_owned(),
                runs: merged(rebase(&runs, &(start..cut))),
                // Every row but the one the line starts on, which is what
                // `start` already says.
                indent: if start > 0 { indent } else { 0 },
            });
            start = cut;
        }
    }
    out
}

/// One run per span, and the whole line plain where the highlighter gave none.
fn runs_of(spans: &[Span], len: usize) -> Vec<Run> {
    let mut out = Vec::with_capacity(spans.len().max(1));
    let mut at = 0usize;
    for span in spans {
        let take = span.len.min(len - at);
        push(&mut out, take, Some(span.class));
        at += take;
        if at == len {
            break;
        }
    }
    push(&mut out, len - at, Some(Class::Plain));
    out
}

/// `runs` with adjacent runs of one class folded together, which a clip can
/// leave behind.
///
/// The core folds spans the same way and keeps it private, which is where it
/// belongs: eight lines shared across a crate boundary costs the core a public
/// item, and this one folds a class the core has no word for.
fn merged(runs: Vec<Run>) -> Vec<Run> {
    let mut out = Vec::with_capacity(runs.len());
    for run in runs {
        push(&mut out, run.len, run.class);
    }
    out
}

/// Append `len` bytes of `class`, merging into the run before it when they agree.
fn push(runs: &mut Vec<Run>, len: usize, class: Option<Class>) {
    if len == 0 {
        return;
    }
    match runs.last_mut() {
        Some(last) if last.class == class => last.len += len,
        _ => runs.push(Run { len, class }),
    }
}
