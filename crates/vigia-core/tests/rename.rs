//! A removal and an addition `gix` left unpaired, paired as a rename by git's rule. `SPEC.md` §11.1.

mod support;

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
    // Past 1,000 times 1,000 nothing is weighed, as git weighs nothing.
    let files: Vec<(String, Vec<u8>)> = (0..1001)
        .map(|n| (format!("old/{n:04}.txt"), format!("old {n}\n").into_bytes()))
        .collect();
    let written: Vec<(String, Vec<u8>)> = (0..1001)
        .map(|n| (format!("new/{n:04}.txt"), format!("new {n}\n").into_bytes()))
        .collect();
    let scratch = moved("rename-limit-past", &refs(&files), &refs(&written));
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    assert_eq!(
        scored(&mut frame),
        0,
        "a walk past the limit weighed a pair"
    );
}

#[test]
fn pair_scores_start_again() {
    let scratch = moved("rename-epoch", &[("a.txt", b"a\n".to_vec())], &[]);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    assert_eq!(frame.pair_epoch(), 0);
    for n in 0..4097u64 {
        frame.learn_pair((n, n), 0);
    }
    assert_eq!(
        frame.pair_epoch(),
        1,
        "4,097 scores did not start the record again"
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
}
