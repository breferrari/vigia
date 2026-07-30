use gix::diff::blob::{Algorithm, InternedInput, diff_with_slider_heuristics};

/// Lines of unchanged context kept on each side of a change.
///
/// Three is git's default and what every reader's eye is calibrated to.
pub const CONTEXT: u32 = 3;

/// How many leading bytes are inspected when deciding if content is binary.
///
/// Git uses the same 8000-byte window, so vigia agrees with `git diff` about
/// what counts as binary.
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
    ///
    /// Invalid UTF-8 is replaced rather than rejected: a monitor that blanks
    /// out because one file is latin-1 has failed at its job.
    pub text: String,
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
    /// Hunks in file order.
    pub hunks: Vec<Hunk>,
    /// Total added lines.
    pub added: u32,
    /// Total removed lines.
    pub removed: u32,
    /// Bytes compared: index-side content plus worktree-side content.
    ///
    /// Recorded because I2 is a claim about work being proportional to edit
    /// size rather than file size, and that is only checkable against a byte
    /// count.
    pub bytes: u64,
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
///
/// Buffers one file, not the whole diff: I4 requires first paint to be
/// independent of *total* diff size, and callers reach this one file at a time.
pub(crate) fn compute(path: String, before: &[u8], after: &[u8]) -> FileDiff {
    let bytes = (before.len() + after.len()) as u64;

    if is_binary(before) || is_binary(after) {
        return FileDiff {
            path,
            binary: true,
            hunks: Vec::new(),
            added: 0,
            removed: 0,
            bytes,
        };
    }

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

    // Raw changes come out in file order. Two of them share an output hunk
    // when their context windows would touch or overlap, which is what stops
    // a run of small edits rendering as a wall of near-duplicate headers.
    let mut group: Vec<gix::diff::blob::Hunk> = Vec::new();
    let flush = |group: &mut Vec<gix::diff::blob::Hunk>,
                 hunks: &mut Vec<Hunk>,
                 added: &mut u32,
                 removed: &mut u32| {
        let Some(first) = group.first() else { return };
        let last = group.last().expect("non-empty");

        let old_start = first.before.start.saturating_sub(CONTEXT);
        let old_end = (last.before.end + CONTEXT).min(before_len);
        let new_start = first.after.start.saturating_sub(CONTEXT);
        let new_end = (last.after.end + CONTEXT).min(after_len);

        let mut lines = Vec::new();
        let mut o = old_start;
        for raw in group.iter() {
            while o < raw.before.start {
                lines.push(Line {
                    kind: LineKind::Context,
                    text: line_before(o),
                });
                o += 1;
            }
            for i in raw.before.clone() {
                lines.push(Line {
                    kind: LineKind::Removed,
                    text: line_before(i),
                });
                *removed += 1;
            }
            for i in raw.after.clone() {
                lines.push(Line {
                    kind: LineKind::Added,
                    text: line_after(i),
                });
                *added += 1;
            }
            o = raw.before.end;
        }
        while o < old_end {
            lines.push(Line {
                kind: LineKind::Context,
                text: line_before(o),
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
        group.clear();
    };

    for raw in diff.hunks() {
        if let Some(last) = group.last() {
            let gap = raw.before.start.saturating_sub(last.before.end);
            if gap > CONTEXT * 2 {
                flush(&mut group, &mut hunks, &mut added, &mut removed);
            }
        }
        group.push(raw);
    }
    flush(&mut group, &mut hunks, &mut added, &mut removed);

    FileDiff {
        path,
        binary: false,
        hunks,
        added,
        removed,
        bytes,
    }
}
