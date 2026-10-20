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
/// The most candidate bytes one frame reads. A pair past it is left to the
/// picture worker.
pub(crate) const FRAME_BUDGET: u64 = 256 * 1024;
/// What scoring one pair costs that budget beyond its sides' bytes, so a
/// frame scores at most 256 pairs however small.
pub(crate) const PAIR_COST: u64 = 1024;
/// The least any read of a side costs that budget, a sniff's length, since
/// what a small read costs is opening the file.
pub(crate) const READ_COST: u64 = crate::hunk::BINARY_SNIFF_LEN as u64;
/// The most pairs one frame lists for the worker.
pub(crate) const LISTED_MAX: usize = (FRAME_BUDGET / PAIR_COST) as usize;

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
    Spans::of(old).score(&Spans::of(new))
}

/// One side cut into git's chunks: how many bytes each chunk hash covers,
/// sorted by hash, as git's `hash_chars` leaves them.
pub(crate) struct Spans {
    binary: bool,
    len: u64,
    counts: Vec<(u32, u64)>,
}

impl Spans {
    /// `data`'s chunks. A text side drops the CR of a CRLF.
    pub(crate) fn of(data: &[u8]) -> Self {
        let binary = crate::hunk::is_binary(data);
        let mut hashes = Vec::new();
        let (mut first, mut second, mut length) = (0u32, 0u32, 0u64);
        for (at, &byte) in data.iter().enumerate() {
            if !binary && byte == b'\r' && data.get(at + 1) == Some(&b'\n') {
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
            hashes.push((hash(first, second), length));
            (first, second, length) = (0, 0, 0);
        }
        if length > 0 {
            hashes.push((hash(first, second), length));
        }
        hashes.sort_unstable_by_key(|&(hash, _)| hash);
        let mut counts: Vec<(u32, u64)> = Vec::with_capacity(hashes.len());
        for (hash, length) in hashes {
            match counts.last_mut() {
                Some(last) if last.0 == hash => last.1 += length,
                _ => counts.push((hash, length)),
            }
        }
        Self {
            binary,
            len: data.len() as u64,
            counts,
        }
    }

    /// Git's score of this side, the old one, against `new`.
    pub(crate) fn score(&self, new: &Self) -> u64 {
        if !self.binary && !new.binary {
            return 0;
        }
        let max = self.len.max(new.len);
        if !sizes_fit(self.len, new.len) {
            return 0;
        }
        let (mut copied, mut theirs) = (0, new.counts.iter().peekable());
        for &(hash, count) in &self.counts {
            while theirs.next_if(|&&(other, _)| other < hash).is_some() {}
            if let Some(&&(other, kept)) = theirs.peek()
                && other == hash
            {
                copied += count.min(kept);
            }
        }
        copied * MAX_SCORE / max
    }
}

/// A chunk's hash, under git's prime.
fn hash(first: u32, second: u32) -> u32 {
    const HASHBASE: u32 = 107_927;
    first.wrapping_add(second.wrapping_mul(0x61)) % HASHBASE
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
