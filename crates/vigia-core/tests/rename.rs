//! A removal and an addition `gix` left unpaired, paired as a rename by git's rule. `SPEC.md` §11.1.

mod support;

use std::time::{Duration, SystemTime};

use support::{Scratch, changes_sorted, noise, tail_changed};
use vigia_core::{ChangeKind, ChangeOptions, FileChange, Frame, Origin, pair_score};

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

/// [`moved`] for a fixture of thousands of files: the removed ones go
/// straight into the object database, never onto disk or through `git add`.
fn moved_large(name: &str, files: &[(&str, Vec<u8>)], written: &[(&str, Vec<u8>)]) -> Scratch {
    use std::io::Write;
    let scratch = Scratch::new(name);
    let mut stream =
        b"commit refs/heads/main\ncommitter vigia <vigia@example.com> 0 +0000\ndata 7\ninitial\n"
            .to_vec();
    for (path, bytes) in files {
        stream.extend(format!("M 100644 inline {path}\ndata {}\n", bytes.len()).as_bytes());
        stream.extend(bytes);
        stream.push(b'\n');
    }
    let mut import = std::process::Command::new("git")
        .args(["fast-import", "--quiet"])
        .current_dir(scratch.path_of(""))
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("git fast-import runs");
    import
        .stdin
        .take()
        .expect("its stdin")
        .write_all(&stream)
        .expect("the stream");
    assert!(
        import.wait().expect("it ends").success(),
        "fast-import failed"
    );
    // The index from that commit, the files it names absent from disk.
    scratch.git(&["reset", "-q"]);
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

/// What `git diff --cached -M --name-status` says about the index, as
/// (status, path) pairs, with a rename's score dropped.
fn git_staged(scratch: &Scratch) -> Vec<(String, String)> {
    let out = scratch.git(&["diff", "--cached", "-M", "--name-status", "-z"]);
    let mut fields = out.split('\0').filter(|field| !field.is_empty());
    let mut found = Vec::new();
    while let Some(status) = fields.next() {
        let letter = status[..1].to_owned();
        if letter == "R" {
            let from = fields.next().expect("a source");
            let to = fields.next().expect("a destination");
            found.push((format!("R {from}"), to.to_owned()));
        } else {
            found.push((letter, fields.next().expect("a path").to_owned()));
        }
    }
    found.sort();
    found
}

/// The staged run's changes.
fn staged(scratch: &Scratch) -> Vec<FileChange> {
    let worktree = scratch.worktree();
    worktree
        .changes_of(Origin::Staged, ChangeOptions::default())
        .expect("a walk")
        .collect::<Result<_, _>>()
        .expect("every change")
}

#[test]
fn text_rename_long_lines() {
    // Long lines each changed at the end: git's chunks end every 64 bytes, so
    // most of each line is kept, where gix's line diff keeps none.
    let line = |n: usize, tail: &str| format!("{}{n:04}{tail}\n", "x".repeat(200));
    let old: String = (0..100).map(|n| line(n, "old")).collect();
    let new: String = (0..100).map(|n| line(n, "new")).collect();
    let scratch = moved(
        "rename-text",
        &[("a.txt", old.into_bytes())],
        &[("b.txt", new.into_bytes())],
    );
    scratch.git(&["add", "-A"]);
    assert_eq!(
        git_staged(&scratch),
        [("R a.txt".to_owned(), "b.txt".to_owned())],
        "the fixture is wrong"
    );
    assert_eq!(kind_of(&staged(&scratch), "b.txt"), &renamed("a.txt"));
}

#[test]
fn text_rename_reordered() {
    // Eight of twelve lines kept in another order: git counts them wherever
    // they are; a line diff counts most as removed.
    let lines: Vec<String> = (0..12)
        .map(|n| format!("line {n} of the original\n"))
        .collect();
    let old: String = lines.concat();
    let mut kept: Vec<String> = lines[..8].to_vec();
    kept.reverse();
    let new: String = kept.concat() + &(0..4).map(|n| format!("fresh {n}\n")).collect::<String>();
    let scratch = moved(
        "rename-reordered",
        &[("a.txt", old.into_bytes())],
        &[("new.txt", new.into_bytes())],
    );
    scratch.git(&["add", "-A"]);
    assert_eq!(
        git_staged(&scratch),
        [("R a.txt".to_owned(), "new.txt".to_owned())],
        "the fixture is wrong"
    );
    assert_eq!(kind_of(&staged(&scratch), "new.txt"), &renamed("a.txt"));
}

#[test]
fn text_rename_diff_unset() {
    // A side whose attributes unset `diff` is binary to git, and git still
    // scores the pair.
    let lines: Vec<String> = (0..12).map(|n| format!("note {n}\n")).collect();
    let mut kept: Vec<String> = lines[..9].to_vec();
    kept.reverse();
    let scratch = moved(
        "rename-diff-unset",
        &[("d.md", lines.concat().into_bytes())],
        &[("new.txt", (kept.concat() + "added\n").into_bytes())],
    );
    scratch.write(".gitattributes", "*.md -diff\n");
    scratch.git(&["add", "-A"]);
    assert!(
        git_staged(&scratch).contains(&("R d.md".to_owned(), "new.txt".to_owned())),
        "the fixture is wrong: {:?}",
        git_staged(&scratch)
    );
    assert_eq!(kind_of(&staged(&scratch), "new.txt"), &renamed("d.md"));
}

#[test]
fn text_rename_basename_first() {
    // The same file name on both sides pairs first when it scores past git's
    // basename bar, though another addition scores higher.
    let lines: Vec<String> = (0..20)
        .map(|n| format!("entry {n} with some text\n"))
        .collect();
    let old = lines.concat();
    // Reversed, so gix's line diff leaves both to vigia.
    let reversed = |kept: usize| lines[..kept].iter().rev().cloned().collect::<String>();
    let near = reversed(17) + "x\ny\nz\n";
    let closer = reversed(19) + "w\n";
    let scratch = moved(
        "rename-basename",
        &[("d0/notes.txt", old.into_bytes())],
        &[
            ("d1/notes.txt", near.into_bytes()),
            ("d1/other.txt", closer.into_bytes()),
        ],
    );
    scratch.git(&["add", "-A"]);
    assert!(
        git_staged(&scratch).contains(&("R d0/notes.txt".to_owned(), "d1/notes.txt".to_owned())),
        "the fixture is wrong: {:?}",
        git_staged(&scratch)
    );
    assert_eq!(
        kind_of(&staged(&scratch), "d1/notes.txt"),
        &renamed("d0/notes.txt")
    );
}

#[test]
fn text_rename_basename_bar() {
    // The same file name below git's basename bar earns no preference, so the
    // better match elsewhere wins.
    let lines: Vec<String> = (0..20)
        .map(|n| format!("entry {n} with some text\n"))
        .collect();
    let reversed = |kept: usize| lines[..kept].iter().rev().cloned().collect::<String>();
    let same_name = reversed(12) + &(0..8).map(|n| format!("new {n}\n")).collect::<String>();
    let elsewhere = reversed(18) + "a\nb\n";
    let scratch = moved(
        "rename-basename-bar",
        &[("d0/notes.txt", lines.concat().into_bytes())],
        &[
            ("d1/notes.txt", same_name.into_bytes()),
            ("d1/other.txt", elsewhere.into_bytes()),
        ],
    );
    scratch.git(&["add", "-A"]);
    assert!(
        git_staged(&scratch).contains(&("R d0/notes.txt".to_owned(), "d1/other.txt".to_owned())),
        "the fixture is wrong: {:?}",
        git_staged(&scratch)
    );
    assert_eq!(
        kind_of(&staged(&scratch), "d1/other.txt"),
        &renamed("d0/notes.txt")
    );
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
fn rename_limit_product() {
    // Git's limit is removals times additions: two removals and 1,001
    // additions are inside it, so the move is paired, as git pairs it.
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
    assert!(
        git_unstaged(&scratch).contains(&("R a.bin".to_owned(), "b.bin".to_owned())),
        "the fixture is wrong"
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    for _ in 0..3 {
        frame.advance().expect("advance");
    }
    assert_eq!(kind_of(frame.files(), "b.bin"), &renamed("a.bin"));
}

#[test]
fn rename_limit_past() {
    // Past 1,000 times 1,000 nothing is weighed, as git weighs nothing: not
    // a side is sized, so the first frame says what every later one would.
    let files: Vec<(String, Vec<u8>)> = (0..1001)
        .map(|n| {
            (
                format!("old/o{n:04}.txt"),
                format!("old {n}\n").into_bytes(),
            )
        })
        .collect();
    let written: Vec<(String, Vec<u8>)> = (0..1001)
        .map(|n| {
            (
                format!("new/n{n:04}.txt"),
                format!("new {n}\n").into_bytes(),
            )
        })
        .collect();
    let scratch = moved_large("rename-limit-past", &refs(&files), &refs(&written));
    let git = git_unstaged(&scratch);
    assert!(
        !git.iter().any(|(status, _)| status.starts_with('R')),
        "the fixture is wrong: git paired {git:?}"
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    assert_eq!(
        scored(&mut frame),
        0,
        "a walk past the limit weighed a pair"
    );
    assert!(
        frame.unscored_pairs().is_empty(),
        "a walk past the limit listed a pair"
    );
    assert_eq!(
        frame.stats().side_reads,
        0,
        "a walk past the limit sized a side"
    );
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
    // Each side is read whole once to score it: the frame pays for each side
    // once.
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
fn text_rename_crlf_diff_unset() {
    // Git keeps the CR of a CRLF on a side whose attributes unset `diff`, and
    // drops it on a text side, so the two sides' chunks differ.
    let lines: Vec<String> = (0..12).map(|n| format!("line {n}\r\n")).collect();
    let mut changed = lines.clone();
    changed[5] = "changed\r\n".to_owned();
    let scratch = moved(
        "rename-crlf-unset",
        &[("d.md", lines.concat().into_bytes())],
        &[("new.txt", changed.concat().into_bytes())],
    );
    scratch.write(".gitattributes", "*.md -diff\n");
    scratch.git(&["add", "-A"]);
    let git = git_staged(&scratch);
    let paired = git.contains(&("R d.md".to_owned(), "new.txt".to_owned()));
    let ours = kind_of(&staged(&scratch), "new.txt").clone();
    assert_eq!(
        ours == renamed("d.md"),
        paired,
        "git {} the pair ({git:?}) and this reported {ours:?}",
        if paired { "paired" } else { "did not pair" }
    );
}

/// What git says about the worktree against the index, renames included:
/// everything staged into a scratch index, diffed against the real one's tree.
fn git_unstaged(scratch: &Scratch) -> Vec<(String, String)> {
    let dir = scratch.path_of("");
    let tree = scratch.git(&["write-tree"]);
    let index = scratch.path_of(".git/scratch-index");
    std::fs::copy(scratch.path_of(".git/index"), &index).expect("copy the index");
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(&dir)
            .env("GIT_INDEX_FILE", &index)
            .output()
            .expect("git runs");
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    git(&["add", "-A"]);
    let out = git(&[
        "diff-index",
        "--cached",
        "-M",
        "--name-status",
        "-z",
        tree.trim(),
    ]);
    let mut fields = out.split('\0').filter(|field| !field.is_empty());
    let mut found = Vec::new();
    while let Some(status) = fields.next() {
        let letter = status[..1].to_owned();
        if letter == "R" {
            let from = fields.next().expect("a source");
            let to = fields.next().expect("a destination");
            found.push((format!("R {from}"), to.to_owned()));
        } else {
            found.push((letter, fields.next().expect("a path").to_owned()));
        }
    }
    found.sort();
    found
}

#[test]
fn four_candidates_per_addition() {
    // Git weighs only an addition's four best removals. Every addition's best
    // four here are r1 to r4, so r5, a fair match for all of them, pairs with
    // none.
    let base = noise(90, 4_000);
    let head_changed = |share: f64| {
        let mut out = base.clone();
        let to = (out.len() as f64 * share) as usize;
        for byte in &mut out[..to] {
            *byte ^= 0x3c;
        }
        out
    };
    let removals: Vec<(String, Vec<u8>)> = (1..=4)
        .map(|i| {
            (
                format!("r{i}.bin"),
                tail_changed(&base, 0.005 * f64::from(i)),
            )
        })
        .chain([("r5.bin".to_owned(), tail_changed(&base, 0.4))])
        .collect();
    let additions: Vec<(String, Vec<u8>)> = (1..=5)
        .map(|j| (format!("a{j}.bin"), head_changed(0.004 * f64::from(j))))
        .collect();
    let scratch = moved("rename-four", &refs(&removals), &refs(&additions));
    let git = git_unstaged(&scratch);
    assert!(
        git.contains(&("D".to_owned(), "r5.bin".to_owned()))
            && git
                .iter()
                .filter(|(status, _)| status.starts_with('R'))
                .count()
                == 4,
        "the fixture is wrong: {git:?}"
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    for _ in 0..4 {
        frame.advance().expect("advance");
    }
    assert!(
        !frame
            .files()
            .iter()
            .any(|change| change.kind == renamed("r5.bin")),
        "r5.bin paired where git leaves it, in {:?}",
        frame.files()
    );
}

#[test]
fn text_rename_autocrlf() {
    // With autocrlf, a worktree file's CRs are not what git scores: it reads
    // the file as it would store it. Near git's threshold they decide the pair.
    let lines: Vec<String> = (0..20).map(|n| format!("row {n:02} x\n")).collect();
    let kept: String = lines[..11].iter().rev().cloned().collect();
    let new: String = kept + &(0..9).map(|n| format!("added {n} y\n")).collect::<String>();
    let scratch = Scratch::new("rename-autocrlf");
    scratch.git(&["config", "core.autocrlf", "true"]);
    scratch.write("a.txt", lines.concat());
    scratch.commit_all("initial");
    std::fs::remove_file(scratch.path_of("a.txt")).expect("remove");
    scratch.write("b.txt", new.replace('\n', "\r\n"));
    let git = git_unstaged(&scratch);
    let paired = git.contains(&("R a.txt".to_owned(), "b.txt".to_owned()));
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    for _ in 0..3 {
        frame.advance().expect("advance");
    }
    let ours = kind_of(frame.files(), "b.txt").clone();
    assert!(
        paired,
        "the fixture is wrong: git does not pair it: {git:?}"
    );
    assert_eq!(ours, renamed("a.txt"));
}

#[cfg(unix)]
#[test]
fn symlink_never_renamed() {
    // Git scores regular files only, so a symlink removed beside a file
    // holding much the same text as its target is no rename.
    let target: String = (0..20).map(|n| format!("segment{n:02}/")).collect();
    let scratch = Scratch::new("rename-symlink");
    std::os::unix::fs::symlink(&target, scratch.path_of("link")).expect("symlink");
    scratch.commit_all("initial");
    std::fs::remove_file(scratch.path_of("link")).expect("remove");
    scratch.write("plain.txt", format!("{target}x"));
    let git = git_unstaged(&scratch);
    assert!(
        git.contains(&("D".to_owned(), "link".to_owned())),
        "the fixture is wrong: {git:?}"
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    for _ in 0..3 {
        frame.advance().expect("advance");
    }
    assert_eq!(kind_of(frame.files(), "plain.txt"), &ChangeKind::Added);
    scratch.git(&["add", "-A"]);
    assert_eq!(kind_of(&staged(&scratch), "plain.txt"), &ChangeKind::Added);
}

#[cfg(unix)]
#[test]
fn executable_rename() {
    // An executable is a regular file to git, so one moved and edited pairs,
    // in either run.
    use std::os::unix::fs::PermissionsExt;
    let old = noise(91, 4_000);
    let scratch = Scratch::new("rename-executable");
    let executable = |path: &str, bytes: &[u8]| {
        scratch.write(path, bytes);
        std::fs::set_permissions(
            scratch.path_of(path),
            std::fs::Permissions::from_mode(0o755),
        )
        .expect("chmod");
    };
    executable("old/tool.bin", &old);
    scratch.commit_all("initial");
    std::fs::remove_file(scratch.path_of("old/tool.bin")).expect("remove");
    executable("new/tool.bin", &tail_changed(&old, 0.1));
    let moved = ("R old/tool.bin".to_owned(), "new/tool.bin".to_owned());
    assert!(
        git_unstaged(&scratch).contains(&moved),
        "the fixture is wrong: {:?}",
        git_unstaged(&scratch)
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    assert_eq!(
        kind_of(frame.files(), "new/tool.bin"),
        &renamed("old/tool.bin")
    );
    scratch.git(&["add", "-A"]);
    assert_eq!(
        kind_of(&staged(&scratch), "new/tool.bin"),
        &renamed("old/tool.bin")
    );
}

#[test]
fn basename_before_cap() {
    // Git pairs a shared file name before it keeps an addition's four best
    // removals, so x.bin pairs by name though four others score higher.
    let new = noise(92, 4_000);
    let removals: Vec<(String, Vec<u8>)> = (1..=4)
        .map(|i| (format!("r{i}.bin"), tail_changed(&new, 0.01 * f64::from(i))))
        .chain([("d0/x.bin".to_owned(), tail_changed(&new, 0.15))])
        .collect();
    let scratch = moved(
        "rename-basename-cap",
        &refs(&removals),
        &[("d1/x.bin", new)],
    );
    let by_name = ("R d0/x.bin".to_owned(), "d1/x.bin".to_owned());
    assert!(
        git_unstaged(&scratch).contains(&by_name),
        "the fixture is wrong: {:?}",
        git_unstaged(&scratch)
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    for _ in 0..3 {
        frame.advance().expect("advance");
    }
    assert_eq!(kind_of(frame.files(), "d1/x.bin"), &renamed("d0/x.bin"));
}

#[test]
fn basename_past_limit() {
    // Git pairs a shared file name before it applies its limit. Past it
    // still at 1,001 times 1,000, the shared name pairs and nothing else is
    // weighed.
    let shared = noise(93, 4_000);
    let files: Vec<(String, Vec<u8>)> = (0..1001)
        .map(|n| {
            (
                format!("old/o{n:04}.txt"),
                format!("old {n}\n").into_bytes(),
            )
        })
        .chain([("old/shared.bin".to_owned(), shared.clone())])
        .collect();
    let written: Vec<(String, Vec<u8>)> = (0..1000)
        .map(|n| {
            (
                format!("new/n{n:04}.txt"),
                format!("new {n}\n").into_bytes(),
            )
        })
        .chain([("new/shared.bin".to_owned(), tail_changed(&shared, 0.1))])
        .collect();
    let scratch = moved_large("rename-basename-limit", &refs(&files), &refs(&written));
    let by_name = ("R old/shared.bin".to_owned(), "new/shared.bin".to_owned());
    assert!(
        git_unstaged(&scratch).contains(&by_name),
        "the fixture is wrong"
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    assert_eq!(
        kind_of(frame.files(), "new/shared.bin"),
        &renamed("old/shared.bin")
    );
}

#[cfg(unix)]
#[test]
fn symlink_names_count() {
    // A symlink is never scored, but its file name still counts, so x.bin is
    // no shared name and the best score pairs instead.
    let new = noise(94, 4_000);
    let scratch = Scratch::new("rename-symlink-name");
    scratch.write("d0/x.bin", tail_changed(&new, 0.15));
    scratch.write("r.bin", tail_changed(&new, 0.02));
    std::fs::create_dir_all(scratch.path_of("l")).expect("mkdir");
    std::os::unix::fs::symlink("elsewhere", scratch.path_of("l/x.bin")).expect("symlink");
    scratch.commit_all("initial");
    for path in ["d0/x.bin", "r.bin", "l/x.bin"] {
        std::fs::remove_file(scratch.path_of(path)).expect("remove");
    }
    scratch.write("d1/x.bin", &new);
    let best = ("R r.bin".to_owned(), "d1/x.bin".to_owned());
    assert!(
        git_unstaged(&scratch).contains(&best),
        "the fixture is wrong: {:?}",
        git_unstaged(&scratch)
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    for _ in 0..3 {
        frame.advance().expect("advance");
    }
    assert_eq!(kind_of(frame.files(), "d1/x.bin"), &renamed("r.bin"));
}

#[test]
fn limit_after_basename() {
    // Git counts its limit after the file-name pass, so one shared name
    // brings 1,001 by 1,000 within it and m.bin pairs with k.bin. The files
    // are small so git's own matrix is cheap, and the worker's list is
    // answered each frame, as the pane's worker answers it.
    let text = |tag: &str, n: usize| format!("{tag} {n}\n").repeat(60).into_bytes();
    let shared = noise(95, 4_000);
    // Lined, so the edit changes one chunk of five, and the cheapest pair.
    let m: Vec<u8> = noise(96, 100)
        .into_iter()
        .enumerate()
        .map(|(at, byte)| if at % 20 == 19 { b'\n' } else { byte })
        .collect();
    let files: Vec<(String, Vec<u8>)> = (0..999)
        .map(|n| (format!("old/o{n:04}.txt"), text("old", n)))
        .chain([
            ("old/shared.bin".to_owned(), shared.clone()),
            ("old/m.bin".to_owned(), m.clone()),
        ])
        .collect();
    let written: Vec<(String, Vec<u8>)> = (0..998)
        .map(|n| (format!("new/n{n:04}.txt"), text("new", n)))
        .chain([
            ("new/shared.bin".to_owned(), tail_changed(&shared, 0.1)),
            ("new/k.bin".to_owned(), tail_changed(&m, 0.1)),
        ])
        .collect();
    let scratch = moved_large("rename-limit-after", &refs(&files), &refs(&written));
    let found = ("R old/m.bin".to_owned(), "new/k.bin".to_owned());
    assert!(
        git_unstaged(&scratch).contains(&found),
        "the fixture is wrong: {:?}",
        git_unstaged(&scratch)
            .iter()
            .filter(|(status, _)| status.starts_with('R'))
            .collect::<Vec<_>>()
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    for _ in 0..10 {
        frame.advance().expect("advance");
        if kind_of(frame.files(), "new/k.bin") == &renamed("old/m.bin") {
            return;
        }
        for candidate in frame.unscored_pairs() {
            frame.learn_pair(candidate.key, vigia_core::candidate_score(&candidate));
        }
    }
    panic!(
        "k.bin never paired: {:?}",
        kind_of(frame.files(), "new/k.bin")
    );
}

#[test]
fn tie_to_same_name() {
    // Two removals score the same against x.bin, under the file-name bar, and
    // the one of the same name pairs, though the other comes first.
    let old = noise(97, 4_000);
    let scratch = moved(
        "rename-tie",
        &[("a/y.bin", old.clone()), ("b/x.bin", old.clone())],
        &[("c/x.bin", tail_changed(&old, 0.35))],
    );
    let named = ("R b/x.bin".to_owned(), "c/x.bin".to_owned());
    assert!(
        git_unstaged(&scratch).contains(&named),
        "the fixture is wrong: {:?}",
        git_unstaged(&scratch)
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    for _ in 0..3 {
        frame.advance().expect("advance");
    }
    assert_eq!(kind_of(frame.files(), "c/x.bin"), &renamed("b/x.bin"));
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

#[test]
fn candidate_slots() {
    // Git offers each addition every removal in order and keeps four slots,
    // a better one taking the first worst slot. s1 to s4 tie for y.bin and
    // s5 takes s1's slot, so once x.bin has s5, y.bin pairs with s2.
    let lined = |seed: u64, len: usize| -> Vec<u8> {
        noise(seed, len)
            .into_iter()
            .enumerate()
            .map(|(at, byte)| if at % 40 == 39 { b'\n' } else { byte })
            .collect()
    };
    let (p, q, r, u, v) = (
        lined(101, 3_600),
        lined(102, 400),
        lined(103, 400),
        lined(104, 1_600),
        lined(105, 1_800),
    );
    let x = [&p[..], &q].concat();
    let s5 = [&p[..], &r].concat();
    let y = [&p[..2_400], &u].concat();
    let w = [&u[..], &p[..600], &v].concat();
    let removals: Vec<(String, Vec<u8>)> = (1..=4)
        .map(|n| (format!("s{n}.bin"), w.clone()))
        .chain([("s5.bin".to_owned(), s5)])
        .collect();
    let scratch = moved(
        "rename-slots",
        &refs(&removals),
        &[("x.bin", x), ("y.bin", y)],
    );
    let git = git_unstaged(&scratch);
    assert!(
        git.contains(&("R s5.bin".to_owned(), "x.bin".to_owned()))
            && git.contains(&("R s2.bin".to_owned(), "y.bin".to_owned())),
        "the fixture is wrong: {git:?}"
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    for _ in 0..4 {
        frame.advance().expect("advance");
    }
    assert_eq!(kind_of(frame.files(), "x.bin"), &renamed("s5.bin"));
    assert_eq!(kind_of(frame.files(), "y.bin"), &renamed("s2.bin"));
}

#[test]
fn intent_to_add_rename() {
    // `git add -N` leaves a file an addition to `git diff`, which pairs it.
    let old = noise(106, 4_000);
    let scratch = moved(
        "rename-intent",
        &[("a.bin", old.clone())],
        &[("b.bin", tail_changed(&old, 0.1))],
    );
    scratch.git(&["add", "-N", "b.bin"]);
    let git = scratch.git(&["diff", "-M", "--name-status"]);
    assert!(git.starts_with("R"), "the fixture is wrong: {git}");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    for _ in 0..3 {
        frame.advance().expect("advance");
    }
    assert_eq!(kind_of(frame.files(), "b.bin"), &renamed("a.bin"));
}

#[test]
fn candidate_slot_ties() {
    // A tie never takes a slot, and the pairs keep git's slot order: with
    // five equal removals y.bin pairs with s1, and when s1 scores a little
    // lower, s5 takes its slot and pairs ahead of s2.
    let lined = |seed: u64, len: usize| -> Vec<u8> {
        noise(seed, len)
            .into_iter()
            .enumerate()
            .map(|(at, byte)| if at % 40 == 39 { b'\n' } else { byte })
            .collect()
    };
    let (p, u, v) = (lined(111, 2_400), lined(112, 1_600), lined(113, 1_920));
    let y = [&p[..], &u].concat();
    let equal = [&u[..], &p[..600], &v[..1_800]].concat();
    let lower = [&u[..], &p[..480], &v].concat();
    // A removal of the same name too small to score holds the first slot at
    // 0, and s5 takes it.
    let tiny = noise(114, 100);
    for (case, first, paired) in [
        ("equal", ("s1.bin", equal.clone()), "s1.bin"),
        ("lower", ("s1.bin", lower), "s5.bin"),
        ("named", ("a/y.bin", tiny), "s5.bin"),
    ] {
        let removals: Vec<(String, Vec<u8>)> = [(first.0.to_owned(), first.1)]
            .into_iter()
            .chain((2..=5).map(|n| (format!("s{n}.bin"), equal.clone())))
            .collect();
        let scratch = moved(
            &format!("rename-slot-{case}"),
            &refs(&removals),
            &[("y.bin", y.clone())],
        );
        let want = (format!("R {paired}"), "y.bin".to_owned());
        let git = git_unstaged(&scratch);
        assert!(git.contains(&want), "{case}: the fixture is wrong: {git:?}");
        let worktree = scratch.worktree();
        let mut frame = worktree.frame();
        for _ in 0..4 {
            frame.advance().expect("advance");
        }
        assert_eq!(kind_of(frame.files(), "y.bin"), &renamed(paired), "{case}");
    }
}

#[test]
fn candidate_slot_name() {
    // Four removals of other names fill y.bin's slots at one score, and a
    // fifth at the same score but y.bin's name ranks higher and takes the
    // first slot, so it pairs.
    let lined = |seed: u64, len: usize| -> Vec<u8> {
        noise(seed, len)
            .into_iter()
            .enumerate()
            .map(|(at, byte)| if at % 40 == 39 { b'\n' } else { byte })
            .collect()
    };
    let (p, u, v) = (lined(121, 2_400), lined(122, 1_600), lined(123, 1_800));
    let y = [&p[..], &u].concat();
    let equal = [&u[..], &p[..600], &v].concat();
    let removals: Vec<(String, Vec<u8>)> = (1..=4)
        .map(|n| (format!("s{n}.bin"), equal.clone()))
        .chain([("z/y.bin".to_owned(), equal.clone())])
        .collect();
    let scratch = moved("rename-slot-name", &refs(&removals), &[("y.bin", y)]);
    let git = git_unstaged(&scratch);
    assert!(
        git.contains(&("R z/y.bin".to_owned(), "y.bin".to_owned())),
        "the fixture is wrong: {git:?}"
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    for _ in 0..4 {
        frame.advance().expect("advance");
    }
    assert_eq!(kind_of(frame.files(), "y.bin"), &renamed("z/y.bin"));
}

/// [`moved`] with `*.png` under git-lfs; `None` where it is not installed.
fn lfs_moved(
    name: &str,
    files: &[(&str, Vec<u8>)],
    written: &[(&str, Vec<u8>)],
) -> Option<Scratch> {
    let installed = std::process::Command::new("git")
        .args(["lfs", "version"])
        .output()
        .is_ok_and(|out| out.status.success());
    if !installed {
        assert!(std::env::var_os("CI").is_none(), "git-lfs is missing on CI");
        eprintln!("skipped: git-lfs is not installed");
        return None;
    }
    let scratch = Scratch::new(name);
    scratch.git(&["lfs", "install", "--local"]);
    scratch.write(
        ".gitattributes",
        "*.png filter=lfs diff=lfs merge=lfs -text\n",
    );
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
    Some(scratch)
}

/// The kind of `path` after a few frames, unstaged, then staged, each
/// checked against `git`'s entry for it.
fn lfs_kinds(scratch: &Scratch, path: &str, git: (&str, &str)) -> (ChangeKind, ChangeKind) {
    let entry = (git.0.to_owned(), git.1.to_owned());
    assert!(
        git_unstaged(scratch).contains(&entry),
        "the fixture is wrong: {:?}",
        git_unstaged(scratch)
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    for _ in 0..3 {
        frame.advance().expect("advance");
    }
    let unstaged = kind_of(frame.files(), path).clone();
    scratch.git(&["add", "-A"]);
    assert!(
        git_staged(scratch).contains(&entry),
        "the fixture is wrong staged: {:?}",
        git_staged(scratch)
    );
    (unstaged, kind_of(&staged(scratch), path).clone())
}

#[test]
fn lfs_rename_unchanged() {
    // Git scores the pointers it stores, so an LFS file moved unchanged is
    // a rename in both runs.
    let old = noise(130, 300_000);
    let Some(scratch) = lfs_moved(
        "rename-lfs-same",
        &[("a.png", old.clone())],
        &[("b.png", old)],
    ) else {
        return;
    };
    assert_eq!(
        lfs_kinds(&scratch, "b.png", ("R a.png", "b.png")),
        (renamed("a.png"), renamed("a.png"))
    );
}

#[test]
fn lfs_rename_edited() {
    // Two pointers to different objects score under git's bar, so an LFS file
    // moved and edited is no rename, under its own name or another.
    let old = noise(131, 300_000);
    for (case, from, to) in [
        ("other", "a.png", "b.png"),
        ("same", "d0/x.png", "d1/x.png"),
    ] {
        let Some(scratch) = lfs_moved(
            &format!("rename-lfs-edit-{case}"),
            &[(from, old.clone())],
            &[(to, tail_changed(&old, 0.01))],
        ) else {
            return;
        };
        assert_eq!(
            lfs_kinds(&scratch, to, ("A", to)),
            (ChangeKind::Added, ChangeKind::Added),
            "{case}"
        );
    }
}

#[test]
fn lfs_rename_past_budget() {
    // An LFS file past what a walk may compare goes to the worker, which pairs
    // it only when its bytes are the removed object, not when they are close.
    let old = noise(132, 17 * 1024 * 1024);
    for (case, new, git_pairs) in [
        ("same", old.clone(), true),
        ("edited", tail_changed(&old, 0.01), false),
    ] {
        let Some(scratch) = lfs_moved(
            &format!("rename-lfs-large-{case}"),
            &[("a.png", old.clone())],
            &[("b.png", new)],
        ) else {
            return;
        };
        let git = git_unstaged(&scratch);
        assert_eq!(
            git.contains(&("R a.png".to_owned(), "b.png".to_owned())),
            git_pairs,
            "{case}: the fixture is wrong: {git:?}"
        );
        let worktree = scratch.worktree();
        let mut frame = worktree.frame();
        frame.advance().expect("advance");
        let pending = frame.unscored_pairs();
        assert_eq!(pending.len(), 1, "{case}: {pending:?}");
        assert!(
            pending[0].differ_score.is_some(),
            "{case}: listed as an ordinary pair"
        );
        frame.learn_pair(pending[0].key, vigia_core::candidate_score(&pending[0]));
        frame.advance().expect("advance");
        let want = if git_pairs {
            renamed("a.png")
        } else {
            ChangeKind::Added
        };
        assert_eq!(kind_of(frame.files(), "b.png"), &want, "{case}");
    }
}

#[test]
fn lfs_rename_grown() {
    // A file of another size than the stored pointer records is never the
    // object, so it costs no compare.
    let old = noise(133, 300_000);
    let mut grown = old.clone();
    grown.extend(noise(134, 1_000));
    let Some(scratch) = lfs_moved("rename-lfs-grown", &[("a.png", old)], &[("b.png", grown)])
    else {
        return;
    };
    let git = git_unstaged(&scratch);
    assert!(
        git.contains(&("A".to_owned(), "b.png".to_owned())),
        "the fixture is wrong: {git:?}"
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    for _ in 0..3 {
        frame.advance().expect("advance");
    }
    assert_eq!(kind_of(frame.files(), "b.png"), &ChangeKind::Added);
    assert_eq!(
        worktree.lfs_compares(),
        0,
        "a compare ran for a size the pointer rules out"
    );
}

#[test]
fn lfs_rename_unfetched() {
    // An object not fetched cannot be compared, so the move shows as removed
    // and added until it is fetched, and then the move pairs.
    let old = noise(135, 300_000);
    let Some(scratch) = lfs_moved(
        "rename-lfs-unfetched",
        &[("a.png", old.clone())],
        &[("b.png", old)],
    ) else {
        return;
    };
    let objects = scratch.path_of(".git/lfs/objects");
    let away = scratch.path_of(".git/lfs/away");
    std::fs::rename(&objects, &away).expect("unfetch");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    for _ in 0..3 {
        frame.advance().expect("advance");
    }
    assert_eq!(kind_of(frame.files(), "b.png"), &ChangeKind::Added);
    std::fs::rename(&away, &objects).expect("fetch");
    frame.advance().expect("advance");
    assert_eq!(kind_of(frame.files(), "b.png"), &renamed("a.png"));
}

#[test]
fn lfs_rename_pointer_worktree() {
    // A file that is its own pointer, as a clone that skipped the smudge
    // leaves it, is stored as it is: moved and added with `-N`, gix leaves it
    // unpaired, and git pairs it.
    let old = noise(136, 300_000);
    let Some(scratch) = lfs_moved("rename-lfs-pointer", &[("a.png", old)], &[]) else {
        return;
    };
    let pointer = scratch.git(&["show", "HEAD:a.png"]);
    assert!(pointer.starts_with("version https://git-lfs"), "{pointer}");
    scratch.write("b.png", pointer);
    scratch.git(&["add", "-N", "b.png"]);
    let git = scratch.git(&["diff", "-M", "--name-status"]);
    assert!(
        git.starts_with("R100\ta.png\tb.png"),
        "the fixture is wrong: {git}"
    );
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    for _ in 0..3 {
        frame.advance().expect("advance");
    }
    assert_eq!(kind_of(frame.files(), "b.png"), &renamed("a.png"));
}

#[test]
fn lfs_unfetched_quiet() {
    // Moves whose objects are not fetched are scored once and kept, so the
    // frames go quiet rather than score them again on every walk.
    let files: Vec<(String, Vec<u8>)> = (0..20)
        .map(|n| (format!("old/{n:02}.png"), noise(140 + n, 1_000)))
        .collect();
    let written: Vec<(String, Vec<u8>)> = files
        .iter()
        .map(|(path, bytes)| (path.replace("old/", "new/"), bytes.clone()))
        .collect();
    let Some(scratch) = lfs_moved("rename-lfs-unfetched-many", &refs(&files), &refs(&written))
    else {
        return;
    };
    std::fs::rename(
        scratch.path_of(".git/lfs/objects"),
        scratch.path_of(".git/lfs/away"),
    )
    .expect("unfetch");
    // Past the settle margin, so a wake is the rename pass's alone.
    let scratch = scratch.settled();
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    for walks in 1.. {
        frame.advance().expect("advance");
        if frame.settles_in(std::time::SystemTime::now()).is_none() {
            break;
        }
        assert!(walks < 10, "still waking after {walks} walks");
    }
}
