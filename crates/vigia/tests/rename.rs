//! A rename the frame could not afford to score, scored on the worker and
//! paired on a later frame. `SPEC.md` §11.1.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use std::time::{Duration, Instant};

use ratatui_image::picker::Picker;
use support::{Scratch, changes_sorted, noise, tail_changed};
use vigia::pictures::{Inks, Pictures};
use vigia_core::ChangeKind;

#[test]
fn binary_rename_paired_by_worker() {
    let old = noise(1, 2 * 1024 * 1024);
    let new = tail_changed(&old, 0.1);
    let scratch = Scratch::new("rename-worker");
    scratch.write("a.bin", &old);
    scratch.commit_all("initial");
    std::fs::remove_file(scratch.path_of("a.bin")).expect("remove");
    scratch.write("b.bin", &new);

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let kind = |frame: &vigia_core::Frame<'_>, path: &str| {
        frame
            .files()
            .iter()
            .find(|change| change.path == path)
            .map(|change| change.kind.clone())
    };
    assert_eq!(
        kind(&frame, "b.bin"),
        Some(ChangeKind::Added),
        "paired on the frame"
    );

    let mut pictures = Pictures::new(
        Inks {
            canvas: (0, 0, 0),
            changed: (255, 0, 0),
        },
        Picker::halfblocks(),
        Box::new(|| {}),
    );
    pictures.score_pairs(&frame);
    let began = Instant::now();
    let found = loop {
        let found = pictures.found_pairs();
        if !found.is_empty() {
            break found;
        }
        assert!(
            began.elapsed() < Duration::from_secs(30),
            "the worker never answered"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(
        pictures.pair(&mut frame, found).expect("advance"),
        "nothing was paired"
    );
    assert_eq!(
        kind(&frame, "b.bin"),
        Some(ChangeKind::Renamed {
            from: "a.bin".to_owned()
        })
    );
    assert!(
        changes_sorted(&worktree)
            .iter()
            .all(|change| change.path != "a.bin"),
        "a.bin is listed beside its rename"
    );
}

#[test]
fn quiet_pairs_never_wake() {
    // Nothing to score: the rename thread blocks on its channel and wakes
    // the pane not once.
    let scratch = Scratch::new("rename-quiet");
    scratch.write("a.txt", "a\n");
    scratch.commit_all("initial");
    scratch.write("a.txt", "b\n");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let wakes = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let counted = std::sync::Arc::clone(&wakes);
    let mut pictures = Pictures::new(
        Inks {
            canvas: (0, 0, 0),
            changed: (255, 0, 0),
        },
        Picker::halfblocks(),
        Box::new(move || {
            counted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }),
    );
    for _ in 0..20 {
        pictures.score_pairs(&frame);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(pictures.found_pairs().is_empty());
    assert_eq!(
        wakes.load(std::sync::atomic::Ordering::Relaxed),
        0,
        "a pane with no pair to score was woken"
    );
}

#[test]
fn pairs_sent_again_after_scores_restart() {
    let old = noise(3, 2 * 1024 * 1024);
    let scratch = Scratch::new("rename-resend");
    scratch.write("a.bin", &old);
    scratch.commit_all("initial");
    std::fs::remove_file(scratch.path_of("a.bin")).expect("remove");
    scratch.write("b.bin", tail_changed(&old, 0.6));
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut pictures = Pictures::new(
        Inks {
            canvas: (0, 0, 0),
            changed: (255, 0, 0),
        },
        Picker::halfblocks(),
        Box::new(|| {}),
    );
    let answers = |pictures: &mut Pictures| {
        let began = Instant::now();
        loop {
            let found = pictures.found_pairs();
            if !found.is_empty() {
                return found.len();
            }
            assert!(began.elapsed() < Duration::from_secs(30), "no answer");
            std::thread::sleep(Duration::from_millis(5));
        }
    };
    pictures.score_pairs(&frame);
    assert_eq!(answers(&mut pictures), 1);
    // The scores start again: the pair, unscored once more, is sent again.
    for n in 0..4097u64 {
        frame.learn_pair((n, n), 0);
    }
    frame.advance().expect("advance");
    pictures.score_pairs(&frame);
    assert_eq!(answers(&mut pictures), 1, "the pair was not sent again");
}

#[test]
fn unpaired_score_no_walk() {
    // A score under half pairs nothing, so nothing is walked again.
    let scratch = Scratch::new("rename-no-walk");
    scratch.write("a.txt", "a\n");
    scratch.commit_all("initial");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let mut pictures = Pictures::new(
        Inks {
            canvas: (0, 0, 0),
            changed: (255, 0, 0),
        },
        Picker::halfblocks(),
        Box::new(|| {}),
    );
    let walks = frame.stats().walks;
    assert!(!pictures.pair(&mut frame, vec![((1, 2), 10)]).expect("pair"));
    assert_eq!(
        frame.stats().walks,
        walks,
        "a score that pairs nothing walked"
    );
    assert!(
        pictures
            .pair(&mut frame, vec![((3, 4), 59_000)])
            .expect("pair")
    );
    assert_eq!(
        frame.stats().walks,
        walks + 1,
        "a score that pairs did not walk"
    );
}
