//! What of an agent's answer is code, and how a quoted line is drawn.
//!
//! An answer arrives as free text an agent wrote with no schema, so the only
//! code this finds is code the agent declared: a fenced block, and a run between
//! backticks. An indented block is words, because it cannot be told from a list
//! item's continuation, and colouring prose as code reads worse than colouring
//! nothing.

use std::ops::Range;

use vigia_core::{Class, Span};

/// A run of a note row's text and the ink it takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run {
    /// Bytes this run covers.
    pub len: usize,
    /// `None` is the row's own voice, the reader's ink or the answer's. `Some`
    /// is quoted code, in the class a grammar gave it or [`Class::Plain`] where
    /// none did.
    pub class: Option<Class>,
}

/// One stretch of an answer: what the agent wrote as words, or what it fenced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Chunk {
    /// Words, newlines and all, to be broken the way prose is.
    Prose(String),
    /// A fenced block, its fences dropped.
    Code {
        /// What the opening fence called the language, where it called it
        /// anything.
        token: Option<String>,
        /// The block's lines, in order.
        lines: Vec<String>,
    },
}

/// The fewest backticks that open a fence.
const FENCE: usize = 3;

/// What a fence names after it, or `None` where `line` is not one.
fn fence(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    let named = trimmed.trim_start_matches('`');
    (trimmed.len() - named.len() >= FENCE).then(|| named.trim())
}

/// `reply` split into what the agent wrote as words and what it fenced.
///
/// A line of three or more backticks opens a block and the next such line closes
/// it; one left open runs to the end. A reply with no fence in it is one
/// [`Chunk::Prose`] holding the reply exactly, so the answer nobody quoted
/// anything in is broken by the rule it has always been broken by.
#[must_use]
pub fn chunks(reply: &str) -> Vec<Chunk> {
    let mut out = Vec::new();
    let mut prose: Vec<&str> = Vec::new();
    let mut token: Option<String> = None;
    let mut lines: Vec<String> = Vec::new();
    let mut fenced = false;

    for line in reply.split('\n') {
        let marker = fence(line);
        if fenced {
            match marker {
                Some(_) => {
                    out.push(Chunk::Code {
                        token: token.take(),
                        lines: std::mem::take(&mut lines),
                    });
                    fenced = false;
                }
                None => lines.push(line.to_owned()),
            }
        } else if let Some(named) = marker {
            if !prose.is_empty() {
                out.push(Chunk::Prose(prose.join("\n")));
                prose.clear();
            }
            token = (!named.is_empty()).then(|| named.to_owned());
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
/// A backtick opens a run and the next one closes it; anything left unclosed is
/// a character the agent wrote. The run takes no grammar, since a backticked
/// word in an answer is as often a path or a flag as it is code, and no runs at
/// all where the paragraph held no pair.
#[must_use]
pub fn inline(paragraph: &str) -> (String, Vec<Run>) {
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
    if runs.is_empty() {
        return (paragraph.to_owned(), Vec::new());
    }
    push(&mut runs, rest.len(), None);
    out.push_str(rest);
    (out, runs)
}

/// `runs` re-based onto the `piece` of the text they cover, and clipped to it.
#[must_use]
pub fn rebase(runs: &[Run], piece: &Range<usize>) -> Vec<Run> {
    let mut out = Vec::new();
    let mut at = 0usize;
    for run in runs {
        let from = at.max(piece.start);
        let to = (at + run.len).min(piece.end);
        if to > from {
            push(&mut out, to - from, run.class);
        }
        at += run.len;
    }
    out
}

/// A quoted block's lines as rows of at most `room` columns, with `spans` from
/// the grammar covering each line.
///
/// Broken at the column and never at a blank: a blank inside a statement is not
/// somewhere a reader would cut it, and `guard country == .BE else { return }`
/// wrapped as a sentence stops reading as one line of anything. A continuation
/// stands in by the line's own indent, which is the rule a wrapped diff line
/// already follows, and those blanks are written into the row because a note row
/// draws one string where a diff row carries the indent beside it.
#[must_use]
pub fn code_rows(lines: &[String], spans: &[Vec<Span>], room: usize) -> Vec<(String, Vec<Run>)> {
    let mut out = Vec::new();
    for (at, line) in lines.iter().enumerate() {
        let runs = runs_of(spans.get(at).map_or(&[][..], Vec::as_slice), line.len());
        // No room to break into, so the row is the line and the painter clips it.
        if room == 0 {
            out.push((line.clone(), runs));
            continue;
        }
        // There is never a cut per byte, so the line's own length bounds the walk
        // without inventing a number for it.
        let cuts = crate::render::breaks_of(line, room, line.len());
        let indent = crate::render::indent_of(line, room);
        let mut start = 0usize;
        for (piece, cut) in cuts
            .iter()
            .copied()
            .chain(std::iter::once(line.len()))
            .enumerate()
        {
            let mut text = String::new();
            let mut drawn = Vec::new();
            if piece > 0 && indent > 0 {
                text.extend(std::iter::repeat_n(' ', indent));
                push(&mut drawn, indent, Some(Class::Plain));
            }
            text.push_str(&line[start..cut]);
            for run in rebase(&runs, &(start..cut)) {
                push(&mut drawn, run.len, run.class);
            }
            out.push((text, drawn));
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
