//! Which bytes of a changed line actually changed.
//!
//! A removed line and the added line that replaced it usually differ in a few
//! tokens, and painting both whole says less than the diff knows. This module
//! pairs the removal run of a hunk with its addition run and marks, per line,
//! the byte ranges that are not common to its partner. `SPEC.md` §11.2 B18 is
//! the ruling; [#321](https://github.com/breferrari/vigia/issues/321) is the
//! build.
//!
//! The shape is delta's, read from its source rather than reinvented
//! (`docs/research/318-drawing-vocabulary.md` §1.3): lines pair index-wise
//! inside one change block, a pair only qualifies when the token-level edit
//! distance is inside a bound, and the bound is what keeps emphasis from
//! smearing across unrelated lines when a block of code is replaced wholesale.
//! delta ships 0.6 as `--max-line-distance` and this module adopts it as the
//! researched default rather than a taste.
//!
//! **Cost rides the diff path, not the frame path.** Pairing runs once per
//! computed diff, inside [`crate::hunk::compute`], so a settled worktree pays
//! nothing per frame; the numbers are in #321's PR. The token walk is linear
//! and the alignment is one LCS table per pair, quadratic in *tokens of one
//! line*, which [`TOKEN_CAP`] bounds so a minified line cannot buy a
//! megabyte-scale table: past the cap a pair is left unpaired, whole-line
//! emphasis being the honest reading of a line nothing sensible can align.

use std::ops::Range;

/// Most tokens a line may hold and still be aligned.
///
/// The LCS table is `O(n * m)` in tokens; two lines at the cap cost a 160k-cell
/// table of `u16`, transiently, which is the top of what a glance element's
/// preprocessing should spend. A line past the cap is almost certainly
/// generated or minified, and word-level emphasis inside one carries no signal
/// a reader can use at a glance anyway.
const TOKEN_CAP: usize = 400;

/// delta's `--max-line-distance`: the fraction of the two lines' combined
/// tokens that may be off the common subsequence before the pair stops being
/// a pair.
const MAX_DISTANCE: f32 = 0.6;

/// One side's within-line changes: byte ranges of `text` that are not shared
/// with the partner line. Empty means the line is identical to its partner in
/// content terms, which for a changed pair means the difference is invisible
/// at token level (whitespace shape inside tokens is a token too, so in
/// practice: never for a real pair).
pub type Emphasis = Vec<Range<u32>>;

/// Pair `removed[i]` with `added[i]` and mark each side's unshared bytes.
///
/// Returns one entry per input line, in order: first `removed.len()` results
/// for the removed side, then `added.len()` for the added side. Unpaired lines
/// (the tail of the longer run, pairs past [`TOKEN_CAP`], pairs outside
/// [`MAX_DISTANCE`]) get an empty emphasis, which the shell draws as the
/// whole-line wash it always drew.
pub fn mark(removed: &[&str], added: &[&str]) -> (Vec<Emphasis>, Vec<Emphasis>) {
    let mut out_removed = vec![Vec::new(); removed.len()];
    let mut out_added = vec![Vec::new(); added.len()];
    for i in 0..removed.len().min(added.len()) {
        if let Some((r, a)) = pair(removed[i], added[i]) {
            out_removed[i] = r;
            out_added[i] = a;
        }
    }
    (out_removed, out_added)
}

/// Align one pair, or say they are too far apart to be one.
fn pair(removed: &str, added: &str) -> Option<(Emphasis, Emphasis)> {
    let r_tokens = tokens(removed);
    let a_tokens = tokens(added);
    if r_tokens.len() > TOKEN_CAP || a_tokens.len() > TOKEN_CAP {
        return None;
    }
    if r_tokens.is_empty() || a_tokens.is_empty() {
        return None;
    }

    let common = lcs(removed, &r_tokens, added, &a_tokens);
    let total = r_tokens.len() + a_tokens.len();
    let distance = (total - 2 * common.len()) as f32 / total as f32;
    if distance > MAX_DISTANCE {
        return None;
    }

    Some((
        unshared(&r_tokens, common.iter().map(|&(r, _)| r)),
        unshared(&a_tokens, common.iter().map(|&(_, a)| a)),
    ))
}

/// Byte ranges of the tokens whose indices are *not* in `shared`, adjacent
/// ranges merged so one edit reads as one patch rather than confetti.
fn unshared(tokens: &[Range<u32>], shared: impl Iterator<Item = usize>) -> Emphasis {
    let mut keep = vec![true; tokens.len()];
    for index in shared {
        keep[index] = false;
    }
    let mut out: Emphasis = Vec::new();
    for (index, range) in tokens.iter().enumerate() {
        if !keep[index] {
            continue;
        }
        match out.last_mut() {
            // Adjacent or whitespace-separated changed tokens merge: the gap
            // between two changed words is part of the same edit to a reader.
            Some(last) if last.end == range.start => last.end = range.end,
            _ => out.push(range.clone()),
        }
    }
    out
}

/// Token boundaries of `line`, as byte ranges.
///
/// A token is a run of alphanumerics or underscores, or a single other
/// non-whitespace character. Whitespace separates and is nobody's token: a
/// reindented line pairs cleanly and its emphasis is the code that moved, not
/// the spaces in front of it.
fn tokens(line: &str) -> Vec<Range<u32>> {
    let mut out = Vec::new();
    let mut word_start: Option<u32> = None;
    for (at, ch) in line.char_indices() {
        let at = at as u32;
        let is_word = ch.is_alphanumeric() || ch == '_';
        if is_word {
            if word_start.is_none() {
                word_start = Some(at);
            }
            continue;
        }
        if let Some(start) = word_start.take() {
            out.push(start..at);
        }
        if !ch.is_whitespace() {
            out.push(at..at + ch.len_utf8() as u32);
        }
    }
    if let Some(start) = word_start {
        out.push(start..line.len() as u32);
    }
    out
}

/// Longest common subsequence over token *content*, returned as index pairs
/// `(removed_token, added_token)` in order.
fn lcs(
    r_text: &str,
    r_tokens: &[Range<u32>],
    a_text: &str,
    a_tokens: &[Range<u32>],
) -> Vec<(usize, usize)> {
    let text = |source: &str, range: &Range<u32>| {
        source
            .get(range.start as usize..range.end as usize)
            .unwrap_or_default()
            .to_owned()
    };
    let r: Vec<String> = r_tokens.iter().map(|range| text(r_text, range)).collect();
    let a: Vec<String> = a_tokens.iter().map(|range| text(a_text, range)).collect();

    // One flat table, lengths only; u16 is enough because TOKEN_CAP bounds both
    // axes far below it.
    let width = a.len() + 1;
    let mut table = vec![0u16; (r.len() + 1) * width];
    for i in (0..r.len()).rev() {
        for j in (0..a.len()).rev() {
            table[i * width + j] = if r[i] == a[j] {
                table[(i + 1) * width + j + 1] + 1
            } else {
                table[(i + 1) * width + j].max(table[i * width + j + 1])
            };
        }
    }

    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < r.len() && j < a.len() {
        if r[i] == a[j] {
            out.push((i, j));
            i += 1;
            j += 1;
        } else if table[(i + 1) * width + j] >= table[i * width + j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ranges(line: &str, emphasis: &Emphasis) -> Vec<String> {
        emphasis
            .iter()
            .map(|r| line[r.start as usize..r.end as usize].to_owned())
            .collect()
    }

    #[test]
    fn a_renamed_call_emphasises_the_name_and_nothing_else() {
        let (r, a) = mark(
            &["fn draw_frame(&self) -> Frame {"],
            &["fn render_frame(&self) -> Frame {"],
        );
        assert_eq!(
            ranges("fn draw_frame(&self) -> Frame {", &r[0]),
            ["draw_frame"]
        );
        assert_eq!(
            ranges("fn render_frame(&self) -> Frame {", &a[0]),
            ["render_frame"]
        );
    }

    #[test]
    fn a_changed_number_emphasises_the_number() {
        let (r, a) = mark(&["    retries: 3,"], &["    retries: 30,"]);
        assert_eq!(ranges("    retries: 3,", &r[0]), ["3"]);
        assert_eq!(ranges("    retries: 30,", &a[0]), ["30"]);
    }

    #[test]
    fn unrelated_lines_stay_unpaired() {
        let (r, a) = mark(
            &["use std::collections::BTreeMap;"],
            &["let widths = measure(&rows, panel.width);"],
        );
        assert!(r[0].is_empty());
        assert!(a[0].is_empty());
    }

    #[test]
    fn the_longer_runs_tail_is_unpaired() {
        let (r, a) = mark(&["let a = 1;"], &["let a = 2;", "let b = fresh();"]);
        assert_eq!(ranges("let a = 1;", &r[0]), ["1"]);
        assert_eq!(ranges("let a = 2;", &a[0]), ["2"]);
        assert!(
            a[1].is_empty(),
            "the unmatched addition keeps the whole-line wash"
        );
    }

    #[test]
    fn adjacent_changed_tokens_merge_into_one_patch() {
        let (r, a) = mark(&["speed = v * 2;"], &["speed = velocity * scale;"]);
        assert_eq!(ranges("speed = v * 2;", &r[0]), ["v", "2"]);
        assert_eq!(
            ranges("speed = velocity * scale;", &a[0]),
            ["velocity", "scale"]
        );
    }

    #[test]
    fn reindentation_emphasises_nothing() {
        let (r, a) = mark(&["value += 1;"], &["        value += 1;"]);
        assert!(r[0].is_empty(), "whitespace is nobody's token");
        assert!(a[0].is_empty());
    }

    #[test]
    fn a_line_past_the_token_cap_is_left_whole() {
        let long = "x, ".repeat(TOKEN_CAP);
        let longer = format!("{long}y");
        let (r, a) = mark(&[long.as_str()], &[longer.as_str()]);
        assert!(r[0].is_empty());
        assert!(a[0].is_empty());
    }

    #[test]
    fn multibyte_content_pairs_on_character_boundaries() {
        let removed = "let título = \"antigo\";";
        let added = "let título = \"novo\";";
        let (r, a) = mark(&[removed], &[added]);
        assert_eq!(ranges(removed, &r[0]), ["antigo"]);
        assert_eq!(ranges(added, &a[0]), ["novo"]);
    }
}
