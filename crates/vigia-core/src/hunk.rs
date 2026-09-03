use gix::diff::blob::{Algorithm, InternedInput, diff_with_slider_heuristics};

/// Lines of unchanged context kept on each side of a change.
pub const CONTEXT: u32 = 3;

/// How many leading bytes are inspected when deciding if content is binary.
const BINARY_SNIFF_LEN: usize = 8000;

/// The role a line plays in a hunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    /// Unchanged, shown for orientation.
    Context,
    /// Present only in the working tree.
    Added,
    /// Present only in the index.
    Removed,
}

/// A single rendered line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// Whether the line was added, removed, or is context.
    pub kind: LineKind,
    /// Line content with any trailing `\n` or `\r\n` stripped.
    pub text: String,
    /// Byte ranges of `text` that are not shared with this line's partner on the other
    /// side of the change, per the crate's `emphasis` module.
    pub emph: crate::emphasis::Emphasis,
}

/// A contiguous run of changes plus its surrounding context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    /// 1-based first line on the index side. Zero when `old_lines` is 0.
    pub old_start: u32,
    /// Number of index-side lines covered.
    pub old_lines: u32,
    /// 1-based first line on the worktree side. Zero when `new_lines` is 0.
    pub new_start: u32,
    /// Number of worktree-side lines covered.
    pub new_lines: u32,
    /// Context, additions and removals in display order.
    pub lines: Vec<Line>,
}

/// The line-level diff for one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    /// Repository-relative path.
    pub path: String,
    /// True when either side sniffed as binary; `hunks` is then empty.
    pub binary: bool,
    /// Why this file has no hunks, when the reason is that a side of it could
    /// not be read. Empties `hunks` the same way `binary` does, and carries no
    /// path: whatever draws it has one.
    pub unreadable: Option<String>,
    /// Hunks in file order.
    pub hunks: Vec<Hunk>,
    /// Total added lines.
    pub added: u32,
    /// Total removed lines.
    pub removed: u32,
    /// Lines on the working-tree side, which is the whole file rather than
    /// the diff.
    pub lines: u32,
    /// The file's own first line, worktree side, falling back to the index
    /// side for a deletion. `None` for a binary file and for the states this
    /// crate deliberately reads nothing for.
    pub first_line: Option<String>,
    /// Bytes compared: index-side content plus worktree-side content.
    pub bytes: u64,
}

impl FileDiff {
    /// The diff of a file a side of which could not be read: no hunks, and the
    /// reason standing where they would be.
    pub(crate) fn failed(path: String, reason: String) -> Self {
        Self {
            path,
            binary: false,
            unreadable: Some(reason),
            hunks: Vec::new(),
            added: 0,
            removed: 0,
            lines: 0,
            // Nothing was read, so there is no first line and nothing should
            // resolve a grammar from one.
            first_line: None,
            bytes: 0,
        }
    }
}

/// Whether `data` should be treated as binary.
pub(crate) fn is_binary(data: &[u8]) -> bool {
    data[..data.len().min(BINARY_SNIFF_LEN)].contains(&0)
}

fn strip_eol(mut line: &[u8]) -> &[u8] {
    if let [rest @ .., b'\n'] = line {
        line = rest;
    }
    if let [rest @ .., b'\r'] = line {
        line = rest;
    }
    line
}

fn text_of(token: &[u8]) -> String {
    String::from_utf8_lossy(strip_eol(token)).into_owned()
}

/// Compute the hunks between two blobs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileSpan {
    /// Hunks the file diffs into.
    pub hunks: u32,
    /// Rows those hunks hold, context included and headers excluded.
    pub lines: u32,
    /// True when either side sniffed as binary, so there are no hunks to draw.
    pub binary: bool,
    /// True when the read this span would have been taken from failed, so the
    /// screen draws a note where the hunks would be. Carried here and not only
    /// on the diff because this walk decides how tall the file is, and a row
    /// with a note under it is two rows rather than one.
    pub unreadable: bool,
    /// Bytes compared to reach this answer.
    pub bytes: u64,
}

impl From<&FileDiff> for FileSpan {
    /// The span a diff already in hand describes, without reading anything.
    fn from(diff: &FileDiff) -> Self {
        Self {
            hunks: diff.hunks.len() as u32,
            lines: diff.hunks.iter().map(|hunk| hunk.lines.len() as u32).sum(),
            binary: diff.binary,
            unreadable: diff.unreadable.is_some(),
            bytes: 0,
        }
    }
}

/// Merge raw changes into the hunks a reader sees.
fn groups(raw: impl Iterator<Item = gix::diff::blob::Hunk>) -> Vec<Vec<gix::diff::blob::Hunk>> {
    let mut out: Vec<Vec<gix::diff::blob::Hunk>> = Vec::new();
    for raw in raw {
        match out.last_mut() {
            Some(group)
                if raw
                    .before
                    .start
                    .saturating_sub(group.last().expect("non-empty").before.end)
                    <= CONTEXT * 2 =>
            {
                group.push(raw);
            }
            _ => out.push(vec![raw]),
        }
    }
    out
}

/// The bounds one merged group covers on each side.
fn bounds(
    group: &[gix::diff::blob::Hunk],
    before_len: u32,
    after_len: u32,
) -> (u32, u32, u32, u32) {
    let first = group.first().expect("non-empty");
    let last = group.last().expect("non-empty");
    (
        first.before.start.saturating_sub(CONTEXT),
        (last.before.end + CONTEXT).min(before_len),
        first.after.start.saturating_sub(CONTEXT),
        (last.after.end + CONTEXT).min(after_len),
    )
}

/// How tall this file's diff is, without building any of it.
pub(crate) fn measure(before: &[u8], after: &[u8]) -> FileSpan {
    let bytes = (before.len() + after.len()) as u64;
    if is_binary(before) || is_binary(after) {
        return FileSpan {
            hunks: 0,
            lines: 0,
            binary: true,
            unreadable: false,
            bytes,
        };
    }

    let input = InternedInput::new(before, after);
    let diff = diff_with_slider_heuristics(Algorithm::Histogram, &input);
    let before_len = input.before.len() as u32;
    let after_len = input.after.len() as u32;

    let mut span = FileSpan {
        bytes,
        ..FileSpan::default()
    };
    for group in groups(diff.hunks()) {
        let (old_start, old_end, _, _) = bounds(&group, before_len, after_len);
        let added: u32 = group
            .iter()
            .map(|raw| raw.after.end - raw.after.start)
            .sum();
        span.hunks += 1;
        span.lines += (old_end - old_start) + added;
    }
    span
}

pub(crate) fn compute(path: String, before: &[u8], after: &[u8]) -> FileDiff {
    let bytes = (before.len() + after.len()) as u64;

    if is_binary(before) || is_binary(after) {
        return FileDiff {
            path,
            binary: true,
            unreadable: None,
            hunks: Vec::new(),
            added: 0,
            removed: 0,
            // Not "unknown" and not counted.
            lines: 0,
            // The same reasoning one field over: bytes that happen to precede
            // an 0x0A are not a line, and nothing should resolve a grammar
            // from them.
            first_line: None,
            bytes,
        };
    }

    // Worktree side first because it is the file the reader is looking at;
    // the index side only for a deletion, where it is the only side there is.
    let first_line = first_line_of(after).or_else(|| first_line_of(before));

    let input = InternedInput::new(before, after);
    // Histogram plus slider heuristics is what git itself produces, so hunks
    // land on the boundaries a reader expects rather than the first
    // mathematically valid ones.
    let diff = diff_with_slider_heuristics(Algorithm::Histogram, &input);

    let before_len = input.before.len() as u32;
    let after_len = input.after.len() as u32;
    let line_before = |i: u32| text_of(input.interner[input.before[i as usize]]);
    let line_after = |i: u32| text_of(input.interner[input.after[i as usize]]);

    let mut hunks: Vec<Hunk> = Vec::new();
    let mut added = 0u32;
    let mut removed = 0u32;

    // Grouped by the same function [`measure`] uses, so a file's drawn height
    // and its counted height cannot disagree.
    for group in groups(diff.hunks()) {
        let (old_start, old_end, new_start, new_end) = bounds(&group, before_len, after_len);

        let mut lines = Vec::new();
        let mut o = old_start;
        for raw in group.iter() {
            while o < raw.before.start {
                lines.push(Line {
                    kind: LineKind::Context,
                    text: line_before(o),
                    emph: Vec::new(),
                });
                o += 1;
            }
            // One raw hunk is one change block: a removal run followed by the addition
            // run that replaced it, which is exactly the unit [`crate::emphasis`]
            // pairs.
            let removed_texts: Vec<String> = raw.before.clone().map(&line_before).collect();
            let added_texts: Vec<String> = raw.after.clone().map(&line_after).collect();
            let (removed_emph, added_emph) = crate::emphasis::mark(&removed_texts, &added_texts);
            for (text, emph) in removed_texts.into_iter().zip(removed_emph) {
                lines.push(Line {
                    kind: LineKind::Removed,
                    text,
                    emph,
                });
                removed += 1;
            }
            for (text, emph) in added_texts.into_iter().zip(added_emph) {
                lines.push(Line {
                    kind: LineKind::Added,
                    text,
                    emph,
                });
                added += 1;
            }
            o = raw.before.end;
        }
        while o < old_end {
            lines.push(Line {
                kind: LineKind::Context,
                text: line_before(o),
                emph: Vec::new(),
            });
            o += 1;
        }

        let old_lines = old_end - old_start;
        let new_lines = new_end - new_start;
        hunks.push(Hunk {
            // git reports a 0 start for an empty side, and 1-based otherwise.
            old_start: if old_lines == 0 { 0 } else { old_start + 1 },
            old_lines,
            new_start: if new_lines == 0 { 0 } else { new_start + 1 },
            new_lines,
            lines,
        });
    }

    FileDiff {
        path,
        binary: false,
        unreadable: None,
        hunks,
        added,
        removed,
        // Already computed above to bound the last hunk's context, so this is
        // the by-product `FileDiff::lines` documents rather than a second pass
        // over the file.
        lines: after_len,
        first_line,
        bytes,
    }
}

/// The first line of `bytes`, capped at 256 bytes read, or `None` for an
/// empty side.
fn first_line_of(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return None;
    }
    let head = &bytes[..bytes.len().min(256)];
    let end = head.iter().position(|&b| b == b'\n').unwrap_or(head.len());
    let line = head[..end].strip_suffix(b"\r").unwrap_or(&head[..end]);
    Some(String::from_utf8_lossy(line).into_owned())
}

#[cfg(test)]
mod tests {
    //! Plus the one property [`measure`] exists for, below: it agrees with
    //! [`compute`] on every shape, which is what lets a scrollbar be sized from
    //! it without the text.

    //! What [`FileDiff::lines`] counts, tested where the count is made.

    use super::*;

    /// The working-tree side, not the index side, and not their sum.
    #[test]
    fn the_line_count_is_the_working_tree_side_not_the_index_side() {
        let before = b"a\nb\nc\nd\ne\n";
        let after = b"a\nb\n";
        let diff = compute("src/lib.rs".to_owned(), before, after);

        assert_eq!(diff.lines, 2, "counted something other than the worktree");
        assert_eq!(diff.removed, 3, "the fixture did not actually shrink");
    }

    /// And the other direction, so a version that counted `before` cannot pass
    /// both this and the test above by accident.
    #[test]
    fn a_file_that_grew_reports_the_longer_side() {
        let diff = compute("src/lib.rs".to_owned(), b"a\n", b"a\nb\nc\n");

        assert_eq!(diff.lines, 3);
        assert_eq!(diff.added, 2, "the fixture did not actually grow");
    }

    /// A new file has no index side at all, which is the commonest change an
    /// agent makes (`SPEC.md` §11.1) and the one where the two sides differ most.
    #[test]
    fn an_addition_counts_the_whole_new_file() {
        let diff = compute("src/new.rs".to_owned(), b"", b"one\ntwo\nthree\n");

        assert_eq!(diff.lines, 3);
    }

    /// A removal has no working-tree side, so there is nothing to locate a
    /// change within and the count is zero rather than the index's length.
    #[test]
    fn a_removal_reports_no_lines_because_there_is_no_file_left() {
        let diff = compute("src/gone.rs".to_owned(), b"a\nb\nc\n", b"");

        assert_eq!(diff.lines, 0);
        assert_eq!(diff.removed, 3);
    }

    /// A newline in a binary file is a byte that happens to be `0x0A`. Counting
    /// them would hand the heat strip a confident number describing nothing.
    #[test]
    fn a_binary_file_reports_no_line_count() {
        let binary = b"\x00\x01\n\x02\n\x03";
        let diff = compute("assets/banner.jpg".to_owned(), b"", binary);

        assert!(diff.binary, "the fixture did not sniff as binary");
        assert_eq!(diff.lines, 0);
    }

    /// What [`FileDiff::first_line`] captures, case by case: the worktree side
    /// when there is one, the index side for a deletion, nothing for binary,
    /// with the cap and the CRLF strip both exercised.
    #[test]
    fn the_first_line_prefers_the_worktree_and_falls_back_for_a_deletion() {
        let both = compute("a.rs".to_owned(), b"old first\nx\n", b"new first\nx\n");
        assert_eq!(both.first_line.as_deref(), Some("new first"));

        let gone = compute("a.rs".to_owned(), b"#!/bin/sh\nx\n", b"");
        assert_eq!(gone.first_line.as_deref(), Some("#!/bin/sh"));

        let binary = compute("a.bin".to_owned(), b"", b"\x00\x01\n\x02");
        assert_eq!(binary.first_line, None);

        let empty = compute("a.rs".to_owned(), b"", b"");
        assert_eq!(empty.first_line, None);

        let crlf = compute("a.rs".to_owned(), b"", b"first\r\nsecond\r\n");
        assert_eq!(crlf.first_line.as_deref(), Some("first"));
    }

    /// The 256-byte cap, which is what keeps a minified bundle's single line
    /// off every [`FileDiff`] of it.
    #[test]
    fn the_first_line_is_capped_and_never_longer() {
        let long = vec![b'x'; 10_000];
        let diff = compute("bundle.js".to_owned(), b"", &long);
        assert_eq!(diff.first_line.as_ref().map(String::len), Some(256));

        // A cut through a four-byte character: 255 ASCII bytes then one lead
        // byte, which `from_utf8_lossy` replaces with a three-byte U+FFFD. The
        // cap is on bytes read, so the answer is 258 and still bounded.
        let mut split = vec![b'x'; 255];
        split.extend_from_slice("🔥".as_bytes());
        let diff = compute("bundle.js".to_owned(), b"", &split);
        assert_eq!(diff.first_line.as_ref().map(String::len), Some(258));
    }

    /// A file with no trailing newline still counts its last line.
    #[test]
    fn a_file_with_no_trailing_newline_counts_its_last_line() {
        let diff = compute("src/lib.rs".to_owned(), b"", b"a\nb");

        assert_eq!(diff.lines, 2);
    }

    /// An empty working-tree file is zero lines rather than one empty line.
    #[test]
    fn an_empty_file_has_no_lines() {
        let diff = compute("src/empty.rs".to_owned(), b"a\n", b"");

        assert_eq!(diff.lines, 0);
    }
}

#[cfg(test)]
mod spans {
    //! [`measure`] against [`compute`], which is the only thing that makes the
    //! cheap path trustworthy.

    use super::*;

    fn rows(diff: &FileDiff) -> (u32, u32) {
        (
            diff.hunks.len() as u32,
            diff.hunks.iter().map(|h| h.lines.len() as u32).sum(),
        )
    }

    /// Every shape the grouping can produce: no change, one edit, two edits far
    /// enough apart to split, two close enough to merge, a pure addition, a pure
    /// deletion, an empty side, and a file with no trailing newline.
    #[test]
    fn a_measured_span_is_what_a_computed_diff_draws() {
        let long: String = (1..=200)
            .map(|n| {
                format!(
                    "line {n}
"
                )
            })
            .collect();
        let mut edited_near: String = long.clone();
        edited_near = edited_near.replace(
            "line 5
",
            "changed 5
",
        );
        edited_near = edited_near.replace(
            "line 9
",
            "changed 9
",
        );
        let mut edited_far: String = long.clone();
        edited_far = edited_far.replace(
            "line 5
",
            "changed 5
",
        );
        edited_far = edited_far.replace(
            "line 150
",
            "changed 150
",
        );

        let cases: Vec<(&str, Vec<u8>, Vec<u8>)> = vec![
            ("identical", long.clone().into(), long.clone().into()),
            (
                "one edit",
                long.clone().into(),
                long.replace(
                    "line 5
", "changed
",
                )
                .into(),
            ),
            ("two edits, merged", long.clone().into(), edited_near.into()),
            ("two edits, split", long.clone().into(), edited_far.into()),
            ("all additions", Vec::new(), long.clone().into()),
            ("all removals", long.clone().into(), Vec::new()),
            ("both empty", Vec::new(), Vec::new()),
            (
                "no trailing newline",
                b"a
b
c"
                .to_vec(),
                b"a
B
c"
                .to_vec(),
            ),
            (
                "one line each",
                b"a
"
                .to_vec(),
                b"b
"
                .to_vec(),
            ),
            ("binary", vec![0, 1, 2, 0], vec![0, 3, 2, 0]),
        ];

        for (label, before, after) in cases {
            let computed = compute("src/lib.rs".to_owned(), &before, &after);
            let measured = measure(&before, &after);
            assert_eq!(
                (measured.hunks, measured.lines),
                rows(&computed),
                "{label}: measured {measured:?} against a diff of {} hunks",
                computed.hunks.len()
            );
            assert_eq!(
                measured.binary, computed.binary,
                "{label}: binary disagrees"
            );
        }
    }
}
