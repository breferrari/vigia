//! A changed binary paired as a rename, by git's rule. `SPEC.md` §11.1.

mod support;

use support::{Scratch, changes_sorted};
use vigia_core::{ChangeKind, FileChange, Frame, pair_score};

/// `length` bytes of noise from `seed`, a NUL among them, so each reads as
/// binary and two seeds share no chunk.
fn noise(seed: u64, length: usize) -> Vec<u8> {
    let mut state = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
    let mut out: Vec<u8> = (0..length)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect();
    out[0] = 0;
    out
}

/// `bytes` with its last `share` of them changed.
fn tail_changed(bytes: &[u8], share: f64) -> Vec<u8> {
    let mut out = bytes.to_vec();
    let from = out.len() - (out.len() as f64 * share) as usize;
    for byte in &mut out[from..] {
        *byte ^= 0x5a;
    }
    out
}

/// `list` with each path borrowed.
fn refs(list: &[(String, Vec<u8>)]) -> Vec<(&str, Vec<u8>)> {
    list.iter().map(|(p, b)| (p.as_str(), b.clone())).collect()
}

/// A repository holding `files` committed, then each removed and its
/// replacement written, unstaged.
fn moved(name: &str, files: &[(&str, Vec<u8>)], written: &[(&str, Vec<u8>)]) -> Scratch {
    let scratch = Scratch::new(name);
    for (path, bytes) in files {
        scratch.write(path, bytes);
    }
    scratch.commit_all("initial");
    for (path, _) in files {
        std::fs::remove_file(scratch.path_of(path)).expect("remove");
    }
    for (path, bytes) in written {
        scratch.write(path, bytes);
    }
    scratch
}

fn kind_of<'a>(changes: &'a [FileChange], path: &str) -> &'a ChangeKind {
    &changes
        .iter()
        .find(|change| change.path == path)
        .unwrap_or_else(|| panic!("{path} is not listed: {changes:?}"))
        .kind
}

fn renamed(from: &str) -> ChangeKind {
    ChangeKind::Renamed {
        from: from.to_owned(),
    }
}

#[test]
fn binary_rename_best_match() {
    let old = noise(1, 20_000);
    let scratch = moved(
        "rename-best",
        &[("a.bin", old.clone())],
        &[
            ("b.bin", tail_changed(&old, 0.1)),
            ("c.bin", tail_changed(&old, 0.3)),
        ],
    );
    let changes = changes_sorted(&scratch.worktree());
    assert_eq!(kind_of(&changes, "b.bin"), &renamed("a.bin"));
    assert_eq!(kind_of(&changes, "c.bin"), &ChangeKind::Added);
}

#[test]
fn binary_rename_tie_same_name() {
    let old = noise(2, 20_000);
    let new = tail_changed(&old, 0.1);
    let scratch = moved(
        "rename-tie",
        &[("one/x.bin", old)],
        &[("two/w.bin", new.clone()), ("two/x.bin", new)],
    );
    let changes = changes_sorted(&scratch.worktree());
    assert_eq!(kind_of(&changes, "two/x.bin"), &renamed("one/x.bin"));
    assert_eq!(kind_of(&changes, "two/w.bin"), &ChangeKind::Added);
}

#[test]
fn binary_rename_size_gate() {
    // Most of the old bytes kept, in a file more than twice the size.
    let old = noise(3, 10_000);
    let new = [old.clone(), noise(4, 11_000)].concat();
    let scratch = moved("rename-size", &[("a.bin", old)], &[("b.bin", new)]);
    let changes = changes_sorted(&scratch.worktree());
    assert_eq!(kind_of(&changes, "a.bin"), &ChangeKind::Removed);
    assert_eq!(kind_of(&changes, "b.bin"), &ChangeKind::Added);
}

#[test]
fn binary_rename_below_half() {
    let old = noise(5, 20_000);
    let scratch = moved(
        "rename-below",
        &[("a.bin", old.clone())],
        &[("b.bin", tail_changed(&old, 0.6))],
    );
    let changes = changes_sorted(&scratch.worktree());
    assert_eq!(kind_of(&changes, "b.bin"), &ChangeKind::Added);
}

#[test]
fn binary_rename_score() {
    // Git's numbers: identical is all of 60,000, and the share kept scales it.
    let old = noise(6, 64 * 1000);
    assert_eq!(pair_score(&old, &old), 60_000);
    let score = pair_score(&old, &tail_changed(&old, 0.25));
    assert!((44_000..=46_000).contains(&score), "{score}");
    assert_eq!(pair_score(b"text\n", b"text, more\n"), 0, "text is gix's");
}

/// Advance `frame` and say how many candidate bytes it scored.
fn scored(frame: &mut Frame<'_>) -> u64 {
    let before = frame.stats().paired_bytes;
    frame.advance().expect("advance");
    frame.stats().paired_bytes - before
}

#[test]
fn binary_rename_cached() {
    let old = noise(7, 20_000);
    let scratch = moved(
        "rename-cached",
        &[("a.bin", old.clone())],
        &[("b.bin", tail_changed(&old, 0.1))],
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    assert!(scored(&mut frame) > 0, "the first frame scored nothing");
    assert_eq!(scored(&mut frame), 0, "a quiet frame scored again");
    assert_eq!(kind_of(frame.files(), "b.bin"), &renamed("a.bin"));
}

#[test]
fn binary_rename_over_frame_budget() {
    // Six pairs of 200 KB, and every removal weighed against every addition:
    // far past one frame's 1 MiB.
    let olds: Vec<Vec<u8>> = (0..6).map(|n| noise(10 + n, 200_000)).collect();
    let files: Vec<(String, Vec<u8>)> = olds
        .iter()
        .enumerate()
        .map(|(n, bytes)| (format!("old{n}.bin"), bytes.clone()))
        .collect();
    let written: Vec<(String, Vec<u8>)> = olds
        .iter()
        .enumerate()
        .map(|(n, bytes)| (format!("new{n}.bin"), tail_changed(bytes, 0.1)))
        .collect();
    let scratch = moved("rename-budget", &refs(&files), &refs(&written));
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    let mut frames = 0;
    loop {
        let bytes = scored(&mut frame);
        assert!(bytes <= 1024 * 1024, "a frame scored {bytes} bytes");
        frames += 1;
        if frame.unscored_pairs().is_empty() {
            break;
        }
        assert!(frames < 100, "the pairs never finished");
    }
    assert!(frames > 1, "one frame scored every pair");
    for n in 0..6 {
        assert_eq!(
            kind_of(frame.files(), &format!("new{n}.bin")),
            &renamed(&format!("old{n}.bin"))
        );
    }
}

#[test]
fn binary_rename_large_on_worker() {
    let old = noise(20, 2 * 1024 * 1024);
    let scratch = moved(
        "rename-large",
        &[("a.bin", old.clone())],
        &[("b.bin", tail_changed(&old, 0.1))],
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    assert_eq!(
        scored(&mut frame),
        0,
        "a 4 MiB pair was scored on the frame"
    );
    assert_eq!(kind_of(frame.files(), "a.bin"), &ChangeKind::Removed);
    assert_eq!(kind_of(frame.files(), "b.bin"), &ChangeKind::Added);
    // What the picture worker does: read both sides, score, hand it back.
    let pending = frame.unscored_pairs();
    assert_eq!(pending.len(), 1, "{pending:?}");
    let candidate = &pending[0];
    let old = candidate
        .sides
        .before
        .as_ref()
        .and_then(|s| s.read())
        .expect("old");
    let new = candidate
        .sides
        .after
        .as_ref()
        .and_then(|s| s.read())
        .expect("new");
    frame.learn_pair(candidate.key, pair_score(&old, &new));
    frame.advance().expect("advance");
    assert_eq!(kind_of(frame.files(), "b.bin"), &renamed("a.bin"));
    assert!(frame.unscored_pairs().is_empty());
}
