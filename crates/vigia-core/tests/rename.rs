//! A changed binary paired as a rename, by git's rule. `SPEC.md` §11.1.

mod support;

use std::time::{Duration, SystemTime};

use support::{Scratch, changes_sorted, noise, tail_changed};
use vigia_core::{ChangeKind, FileChange, Frame, pair_score};

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
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    // Not read at all: the sizes decide first.
    assert_eq!(scored(&mut frame), 0, "a pair the sizes rule out was read");
    assert_eq!(kind_of(frame.files(), "a.bin"), &ChangeKind::Removed);
    assert_eq!(kind_of(frame.files(), "b.bin"), &ChangeKind::Added);
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
    // Six pairs of 50 KB, and every removal weighed against every addition:
    // far past one frame's 256 KiB.
    let olds: Vec<Vec<u8>> = (0..6).map(|n| noise(10 + n, 50_000)).collect();
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
        assert!(bytes <= 256 * 1024, "a frame scored {bytes} bytes");
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

#[test]
fn binary_rename_smallest_first() {
    // Two small pairs and one large one, more than a frame together: the
    // small ones are scored first.
    let small: Vec<Vec<u8>> = (0..2).map(|n| noise(30 + n, 20_000)).collect();
    // The large pair fits a frame alone, and not beside the small ones.
    let large = noise(40, 100_000);
    let scratch = moved(
        "rename-smallest",
        &[
            ("s0.bin", small[0].clone()),
            ("s1.bin", small[1].clone()),
            ("l.bin", large.clone()),
        ],
        &[
            ("t0.bin", tail_changed(&small[0], 0.1)),
            ("t1.bin", tail_changed(&small[1], 0.1)),
            ("m.bin", tail_changed(&large, 0.1)),
        ],
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    assert_eq!(kind_of(frame.files(), "t0.bin"), &renamed("s0.bin"));
    assert_eq!(kind_of(frame.files(), "t1.bin"), &renamed("s1.bin"));
}

#[test]
fn text_rename_left_to_gix() {
    // Long lines each changed at the end: git's chunk score keeps most of
    // them, and gix's line score keeps none. Text keeps gix's score.
    let line = |n: usize, tail: &str| format!("{}{n:04}{tail}\n", "x".repeat(200));
    let old: String = (0..100).map(|n| line(n, "old")).collect();
    let new: String = (0..100).map(|n| line(n, "new")).collect();
    let old_len = old.len() as u64;
    let scratch = moved(
        "rename-text",
        &[("a.txt", old.into_bytes())],
        &[("b.txt", new.into_bytes())],
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    // Read only to learn the old side is text, and not scored.
    assert_eq!(scored(&mut frame), old_len, "a text pair was scored");
    assert_eq!(kind_of(frame.files(), "b.txt"), &ChangeKind::Added);
}

#[test]
fn binary_rename_old_side_binary() {
    // Binary only before: one NUL at its start, which the new side drops.
    let mut old: Vec<u8> = noise(50, 20_000)
        .into_iter()
        .map(|byte| byte.max(1))
        .collect();
    old[0] = 0;
    let new = old[1..].to_vec();
    let scratch = moved("rename-old-binary", &[("a.dat", old)], &[("b.dat", new)]);
    let changes = changes_sorted(&scratch.worktree());
    assert_eq!(kind_of(&changes, "b.dat"), &renamed("a.dat"));
}

#[test]
fn binary_rename_limit() {
    // 1,001 additions: past what a frame sizes, so nothing is weighed, though
    // the 2,002 pairs are inside git's own limit.
    let old = noise(60, 20_000);
    let mut files = vec![("a.bin", old.clone()), ("gone.txt", b"gone\n".to_vec())];
    files.sort();
    let names: Vec<String> = (0..1000).map(|n| format!("new/{n:04}.txt")).collect();
    let mut written: Vec<(&str, Vec<u8>)> = names
        .iter()
        .map(|name| (name.as_str(), format!("{name}\n").into_bytes()))
        .collect();
    written.push(("b.bin", tail_changed(&old, 0.1)));
    let scratch = moved("rename-limit", &files, &written);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    assert_eq!(
        scored(&mut frame),
        0,
        "a walk past the limit weighed a pair"
    );
    assert_eq!(kind_of(frame.files(), "b.bin"), &ChangeKind::Added);
}

#[test]
fn full_record_makes_room() {
    let old = noise(82, 200_000);
    let scratch = moved(
        "rename-room",
        &[("a.bin", old.clone())],
        &[("b.bin", tail_changed(&old, 0.1))],
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    // Past the frame's budget, the pair waits for the worker.
    let key = frame.unscored_pairs().first().expect("listed").key;
    for n in 0..4096u64 {
        frame.learn_pair((u64::MAX - n, n), 0);
    }
    // The record is full of scores filed this frame, so a.bin's has no room.
    frame.learn_pair(key, 60_000);
    frame.advance().expect("advance");
    assert_eq!(kind_of(frame.files(), "b.bin"), &ChangeKind::Added);
    assert_eq!(frame.pair_epoch(), 0, "a current score was evicted");
    assert_eq!(
        frame.settles_in(SystemTime::now()),
        Some(Duration::ZERO),
        "the pair waits for room and no frame comes to make it"
    );
    assert!(
        frame.pairs_unlisted(),
        "the worker's next answer would not walk again"
    );
    // A frame later the fillers are stale, and the pair is asked for again.
    frame.advance().expect("advance");
    assert_eq!(frame.pair_epoch(), 1);
    let listed: Vec<_> = frame.unscored_pairs().iter().map(|c| c.key).collect();
    assert_eq!(listed, [key]);
    frame.learn_pair(key, 60_000);
    frame.advance().expect("advance");
    assert_eq!(kind_of(frame.files(), "b.bin"), &renamed("a.bin"));
    // Full again, of scores a frame older than the walk, one makes room.
    for n in 0..4095u64 {
        frame.learn_pair((n, u64::MAX - n), 0);
    }
    frame.advance().expect("advance");
    assert_eq!(frame.pair_epoch(), 1);
    assert_eq!(
        frame.settles_in(SystemTime::now()),
        None,
        "no pair waits for room, and a frame was asked for"
    );
    frame.learn_pair((u64::MAX, u64::MAX), 0);
    assert_eq!(
        frame.pair_epoch(),
        2,
        "a score past a record of stale ones did not take one's place"
    );
}

#[test]
fn binary_rename_charged_once() {
    // The old side is read whole to learn it is binary, and that read serves
    // the score too: the frame pays for each side once.
    // Binary only before, so the old side is the one read to learn it.
    let mut old: Vec<u8> = noise(70, 100_000)
        .into_iter()
        .map(|byte| byte.max(1))
        .collect();
    old[0] = 0;
    let new = old[1..].to_vec();
    let total = (old.len() + new.len()) as u64;
    let scratch = moved("rename-once", &[("a.bin", old)], &[("b.bin", new)]);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    assert_eq!(scored(&mut frame), total);
    assert_eq!(kind_of(frame.files(), "b.bin"), &renamed("a.bin"));
}

#[test]
fn binary_rename_small_before_text() {
    // Two large text pairs come first in the walk, and the small binary pair
    // is still paid for first.
    let text = |n: u8| -> Vec<u8> {
        (0..100_000u32)
            .map(|at| b'a' + ((at + u32::from(n)) % 26) as u8)
            .collect()
    };
    // The text removals' reads, 200 KB, would leave too little for it.
    let small = noise(80, 30_000);
    let scratch = moved(
        "rename-small-first",
        &[
            ("a1.txt", text(1)),
            ("a2.txt", text(2)),
            ("z.bin", small.clone()),
        ],
        &[
            ("b1.txt", text(3)),
            ("b2.txt", text(4)),
            ("y.bin", tail_changed(&small, 0.1)),
        ],
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    assert_eq!(kind_of(frame.files(), "y.bin"), &renamed("z.bin"));
}

#[test]
fn staged_count_own_budget() {
    // A burst the frame spends its budget on, unstaged, beside three staged
    // binary renames the staged count weighs on a budget of its own.
    let scratch = Scratch::new("rename-staged-count");
    let olds: Vec<Vec<u8>> = (0..40).map(|n| noise(600 + n, 2_000)).collect();
    for (n, old) in olds.iter().enumerate() {
        scratch.write(&format!("old/{n:02}.bin"), old);
    }
    let staged: Vec<Vec<u8>> = (0..3).map(|n| noise(700 + n, 3_000)).collect();
    for (n, old) in staged.iter().enumerate() {
        scratch.write(&format!("s/{n}.bin"), old);
    }
    scratch.commit_all("initial");
    for (n, old) in staged.iter().enumerate() {
        std::fs::remove_file(scratch.path_of(&format!("s/{n}.bin"))).expect("remove");
        scratch.write(&format!("t/{n}.bin"), tail_changed(old, 0.1));
    }
    scratch.git(&["add", "-A", "s", "t"]);
    for (n, old) in olds.iter().enumerate() {
        std::fs::remove_file(scratch.path_of(&format!("old/{n:02}.bin"))).expect("remove");
        scratch.write(&format!("new/{n:02}.bin"), tail_changed(old, 0.1));
    }
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    for _ in 0..10 {
        frame.advance().expect("advance");
        if frame.settles_in(std::time::SystemTime::now()).is_none() {
            break;
        }
    }
    let listed = frame.unscored_pairs().len();
    let counted = worktree
        .count_of(vigia_core::Origin::Staged, None)
        .expect("count");
    assert_eq!(counted.shown, 3, "the staged renames counted unpaired");
    assert_eq!(
        frame.unscored_pairs().len(),
        listed,
        "the count changed the frame's list"
    );
}

#[test]
fn record_keeps_current_pairs() {
    // A full record of scores for files long gone must not stop a fresh
    // rename from being scored. 4,096 fills it without the restart a 4,097th
    // learned score would trigger.
    let old = noise(81, 20_000);
    let scratch = moved(
        "rename-stale-record",
        &[("a.bin", old.clone())],
        &[("b.bin", tail_changed(&old, 0.1))],
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    for n in 0..4096u64 {
        frame.learn_pair((u64::MAX - n, n), 0);
    }
    // Filed at frame 0, they last the first frame, which asks for the next.
    frame.advance().expect("advance");
    assert_eq!(
        frame.settles_in(SystemTime::now()),
        Some(Duration::ZERO),
        "the pair waits for room and no frame comes to make it"
    );
    frame.advance().expect("advance");
    assert_eq!(kind_of(frame.files(), "b.bin"), &renamed("a.bin"));
    assert_eq!(
        frame.pair_epoch(),
        1,
        "the stale scores went without starting the record again, so the worker \
         would never ask again for a dropped pair"
    );
    // Full again beside that score, the record keeps it.
    let scored = frame.stats().pairs_scored;
    for n in 0..4095u64 {
        frame.learn_pair((n, u64::MAX - n), 0);
    }
    frame.advance().expect("advance");
    assert_eq!(kind_of(frame.files(), "b.bin"), &renamed("a.bin"));
    assert_eq!(
        frame.stats().pairs_scored,
        scored,
        "a full record dropped a score the last frame asked about"
    );
}

#[test]
fn record_over_cap() {
    // 65 by 65 binaries is 4,225 live pairs, more than the record holds. Once
    // it is full of them it must drop none, or the worker scores them again,
    // whether it answers before the next frame or after it.
    for late in [false, true] {
        let olds: Vec<(String, Vec<u8>)> = (0..65)
            .map(|n| (format!("old/{n:02}.bin"), noise(300 + n, 1_000)))
            .collect();
        let news: Vec<(String, Vec<u8>)> = (0..65)
            .map(|n| (format!("new/{n:02}.bin"), noise(400 + n, 1_000)))
            .collect();
        let scratch = moved(
            &format!("rename-over-cap-{late}"),
            &refs(&olds),
            &refs(&news),
        );
        let worktree = scratch.worktree();
        let mut frame = worktree.frame();
        let (mut quiet, mut asked, mut learned) = (0, Vec::new(), 0);
        for _ in 0..200 {
            frame.advance().expect("advance");
            let listed = frame.unscored_pairs();
            let answered = if late {
                std::mem::replace(&mut asked, listed)
            } else {
                listed
            };
            learned += answered.len() as u64;
            for candidate in answered {
                frame.learn_pair(candidate.key, 0);
            }
            if frame.unscored_pairs().is_empty() && asked.is_empty() {
                quiet += 1;
                if quiet == 5 {
                    break;
                }
                continue;
            }
            quiet = 0;
        }
        assert_eq!(
            quiet, 5,
            "late {late}: the frames never stopped listing pairs"
        );
        let scored = frame.stats().pairs_scored;
        assert!(
            learned + scored >= 4096,
            "late {late}: the record stopped at {} scores",
            learned + scored
        );
        for _ in 0..5 {
            frame.advance().expect("advance");
        }
        assert_eq!(
            frame.settles_in(SystemTime::now()),
            None,
            "late {late}: a record of live pairs asked for a frame"
        );
        assert_eq!(
            frame.pair_epoch(),
            0,
            "late {late}: a record of live pairs dropped some, so the worker scores them again"
        );
        assert_eq!(
            frame.stats().pairs_scored,
            scored,
            "late {late}: a full record scored again"
        );
    }
}

#[test]
fn listed_room_no_wake() {
    // Short of full, a record whose room is held by pairs listed for the
    // worker waits for its answers, not for a frame that could drop nothing.
    let olds: Vec<(String, Vec<u8>)> = (0..15)
        .map(|n| (format!("old/{n:02}.bin"), noise(500 + n, 300_000)))
        .collect();
    let news: Vec<(String, Vec<u8>)> = (0..15)
        .map(|n| (format!("new/{n:02}.bin"), noise(600 + n, 300_000)))
        .collect();
    let scratch = moved("rename-listed-room", &refs(&olds), &refs(&news));
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    for n in 0..3900u64 {
        frame.learn_pair((u64::MAX - n, n), 0);
    }
    frame.advance().expect("advance");
    assert_eq!(frame.unscored_pairs().len(), 196, "the fixture is wrong");
    assert!(
        frame.pairs_unlisted(),
        "the worker's answers would not walk again"
    );
    assert_eq!(
        frame.settles_in(SystemTime::now()),
        None,
        "a frame that can make no room was asked for"
    );
}
