//! A changed binary paired as a rename, by git's rule. `gix` never scores a
//! pair with a binary side, so these pairs are scored here.

use crate::picture::Sources;

/// Git's scale for a similarity score.
const MAX_SCORE: u64 = 60_000;
/// Git's default threshold, 50%.
const MINIMUM_SCORE: u64 = MAX_SCORE / 2;
/// The most unpaired removals, and additions, scored against each other:
/// git's default `diff.renameLimit`.
pub(crate) const RENAME_LIMIT: usize = 1000;
/// The most candidate bytes one frame scores. A pair past it is left to the
/// picture worker.
pub(crate) const FRAME_BUDGET: u64 = 256 * 1024;

/// One pair of versions: the removal's blob stamp and the addition's stamp.
pub type PairKey = (u64, u64);

/// A pair the frame could not afford to score, for the picture worker.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// What [`crate::Frame::learn_pair`] files its score under.
    pub key: PairKey,
    /// The removed side, then the added side.
    pub sides: Sources,
}

/// Whether git scores a pair of these sizes at all: one whose sizes differ by
/// more than the threshold allows is not.
pub(crate) fn sizes_fit(a: u64, b: u64) -> bool {
    let (max, min) = (a.max(b), a.min(b));
    max > 0 && max * (MAX_SCORE - MINIMUM_SCORE) >= (max - min) * MAX_SCORE
}

/// Git's similarity of `old` to `new`, on its scale of 60,000: the bytes of
/// `old`'s chunks that `new` keeps, over the larger size. A chunk ends at a
/// line feed or after 64 bytes. Zero when neither side is binary, a pair
/// `gix` scored itself.
pub fn pair_score(old: &[u8], new: &[u8]) -> u64 {
    let (old_text, new_text) = (!crate::hunk::is_binary(old), !crate::hunk::is_binary(new));
    if old_text && new_text {
        return 0;
    }
    let max = old.len().max(new.len()) as u64;
    if !sizes_fit(old.len() as u64, new.len() as u64) {
        return 0;
    }
    let (old_spans, new_spans) = (spans(old, old_text), spans(new, new_text));
    let copied: u64 = old_spans
        .iter()
        .zip(&new_spans)
        .map(|(&old, &new)| old.min(new))
        .sum();
    copied * MAX_SCORE / max
}

/// [`pair_score`] of a candidate's two sides, read on the calling thread; 0
/// when either cannot be read.
pub fn candidate_score(candidate: &Candidate) -> u64 {
    let read = |side: &Option<crate::picture::Source>| side.as_ref().and_then(|s| s.read());
    match (read(&candidate.sides.before), read(&candidate.sides.after)) {
        (Some(old), Some(new)) => pair_score(&old, &new),
        _ => 0,
    }
}

/// Whether a score pairs.
pub fn pairs(score: u64) -> bool {
    score >= MINIMUM_SCORE
}

/// Git's chunk hashes fall under this prime.
const HASHBASE: u32 = 107_927;

/// Bytes per chunk hash, as git's `hash_chars` counts them, indexed by the
/// hash. A text side drops the CR of a CRLF.
fn spans(data: &[u8], text: bool) -> Vec<u64> {
    let mut out = vec![0; HASHBASE as usize];
    let (mut first, mut second, mut length) = (0u32, 0u32, 0u64);
    let mut add = |first: u32, second: u32, length: u64| {
        let hash = first.wrapping_add(second.wrapping_mul(0x61)) % HASHBASE;
        out[hash as usize] += length;
    };
    for (at, &byte) in data.iter().enumerate() {
        if text && byte == b'\r' && data.get(at + 1) == Some(&b'\n') {
            continue;
        }
        let old = first;
        first = (first << 7) ^ (second >> 25);
        second = (second << 7) ^ (old >> 25);
        first = first.wrapping_add(u32::from(byte));
        length += 1;
        if length < 64 && byte != b'\n' {
            continue;
        }
        add(first, second, length);
        (first, second, length) = (0, 0, 0);
    }
    if length > 0 {
        add(first, second, length);
    }
    out
}
