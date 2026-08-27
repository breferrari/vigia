//! Which bytes of a changed line actually changed.

use std::ops::Range;

/// Most tokens a line may hold and still be aligned.
const TOKEN_CAP: usize = 400;

/// delta's `--max-line-distance`: the fraction of the two lines' combined
/// tokens that may be off the common subsequence before the pair stops being
/// a pair.
const MAX_DISTANCE: f32 = 0.6;

/// One side's within-line changes: byte ranges of `text` that are not shared with the
/// partner line.
pub type Emphasis = Vec<Range<u32>>;

/// Pair `removed[i]` with `added[i]` and mark each side's unshared bytes.
pub fn mark<S: AsRef<str>>(removed: &[S], added: &[S]) -> (Vec<Emphasis>, Vec<Emphasis>) {
    let mut out_removed = vec![Vec::new(); removed.len()];
    let mut out_added = vec![Vec::new(); added.len()];
    for i in 0..removed.len().min(added.len()) {
        if let Some((r, a)) = pair(removed[i].as_ref(), added[i].as_ref()) {
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
    // `lcs` emits its indices in strictly ascending order (prefix, middle
    // walk, then the suffix loop reversed back to ascending), so membership is
    // one peek rather than a bool table per pair.
    let mut shared = shared.peekable();
    let mut out: Emphasis = Vec::new();
    for (index, range) in tokens.iter().enumerate() {
        if shared.next_if_eq(&index).is_some() {
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
    fn slice<'t>(source: &'t str, range: &Range<u32>) -> &'t str {
        source
            .get(range.start as usize..range.end as usize)
            .unwrap_or_default()
    }
    let r: Vec<&str> = r_tokens.iter().map(|range| slice(r_text, range)).collect();
    let a: Vec<&str> = a_tokens.iter().map(|range| slice(a_text, range)).collect();

    let mut prefix = 0;
    while prefix < r.len() && prefix < a.len() && r[prefix] == a[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < r.len() - prefix
        && suffix < a.len() - prefix
        && r[r.len() - 1 - suffix] == a[a.len() - 1 - suffix]
    {
        suffix += 1;
    }
    let rm = &r[prefix..r.len() - suffix];
    let am = &a[prefix..a.len() - suffix];

    // One flat table over the disagreeing middle, lengths only; u16 is enough
    // because TOKEN_CAP bounds both axes far below it.
    let width = am.len() + 1;
    let mut table = vec![0u16; (rm.len() + 1) * width];
    for i in (0..rm.len()).rev() {
        for j in (0..am.len()).rev() {
            table[i * width + j] = if rm[i] == am[j] {
                table[(i + 1) * width + j + 1] + 1
            } else {
                table[(i + 1) * width + j].max(table[i * width + j + 1])
            };
        }
    }

    let mut out: Vec<(usize, usize)> = (0..prefix).map(|i| (i, i)).collect();
    let (mut i, mut j) = (0, 0);
    while i < rm.len() && j < am.len() {
        if rm[i] == am[j] {
            out.push((prefix + i, prefix + j));
            i += 1;
            j += 1;
        } else if table[(i + 1) * width + j] >= table[i * width + j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    for k in (0..suffix).rev() {
        out.push((r.len() - 1 - k, a.len() - 1 - k));
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
