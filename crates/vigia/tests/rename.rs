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

#[test]
fn many_small_pairs_finish_unprompted() {
    finish_unprompted("rename-many-unprompted", false);
}

#[test]
fn many_small_staged_finish_unprompted() {
    finish_unprompted("rename-many-staged", true);
}

/// Forty small binaries moved and changed, staged or not: more sides than one
/// frame can weigh, and no event after the first.
fn finish_unprompted(name: &str, staged: bool) {
    let scratch = Scratch::new(name);
    let olds: Vec<Vec<u8>> = (0..40).map(|n| noise(100 + n, 2_000)).collect();
    for (n, old) in olds.iter().enumerate() {
        scratch.write(&format!("old/{n:02}.bin"), old);
    }
    scratch.commit_all("initial");
    for (n, old) in olds.iter().enumerate() {
        std::fs::remove_file(scratch.path_of(&format!("old/{n:02}.bin"))).expect("remove");
        scratch.write(&format!("new/{n:02}.bin"), tail_changed(old, 0.1));
    }
    if staged {
        scratch.git(&["add", "-A"]);
    }
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.show_staged(staged);
    frame.advance().expect("advance");
    let mut pictures = Pictures::new(
        Inks {
            canvas: (0, 0, 0),
            changed: (255, 0, 0),
        },
        Picker::halfblocks(),
        Box::new(|| {}),
    );
    let renamed = |frame: &vigia_core::Frame<'_>| {
        frame
            .files()
            .iter()
            .filter(|change| matches!(change.kind, ChangeKind::Renamed { .. }))
            .count()
    };
    assert!(
        frame.settles_in(std::time::SystemTime::now()).is_some(),
        "one frame weighed every side"
    );
    // What the pane does with no event: its settle wake, then the worker's.
    let began = Instant::now();
    let mut read = (0, 0);
    while renamed(&frame) < 40 {
        let stats = frame.stats();
        assert!(
            stats.side_reads - read.0 <= 32 * (stats.walks - read.1),
            "a frame read {} sides, past 256 KiB at 8 KiB a read",
            stats.side_reads - read.0
        );
        read = (stats.side_reads, stats.walks);
        assert!(
            began.elapsed() < Duration::from_secs(30),
            "the pairs stopped at {} of 40 with no event",
            renamed(&frame)
        );
        let now = std::time::SystemTime::now();
        if let Some(wait) = frame.settles_in(now) {
            std::thread::sleep(wait);
            frame
                .advance_if_settled(std::time::SystemTime::now())
                .expect("advance");
        }
        pictures.score_pairs(&frame);
        std::thread::sleep(Duration::from_millis(5));
        let found = pictures.found_pairs();
        pictures.pair(&mut frame, found).expect("pair");
    }
    assert_eq!(
        frame.settles_in(std::time::SystemTime::now()),
        None,
        "a wake is armed with every pair decided"
    );
    assert!(!frame.pairs_unlisted(), "pairs are left to list");
}

#[test]
fn unlisted_pairs_listed_later() {
    // More unrelated pairs than a frame lists, each cheaper than the one real
    // rename, so the rename is listed only once the worker has answered them.
    let scratch = Scratch::new("rename-unlisted");
    for n in 0..17 {
        scratch.write(&format!("old/{n:02}.bin"), noise(200 + n, 260 * 1024));
    }
    let moved = noise(7, 300 * 1024);
    scratch.write("a.bin", &moved);
    scratch.commit_all("initial");
    for n in 0..17u64 {
        std::fs::remove_file(scratch.path_of(&format!("old/{n:02}.bin"))).expect("remove");
        scratch.write(&format!("new/{n:02}.bin"), noise(300 + n, 260 * 1024));
    }
    std::fs::remove_file(scratch.path_of("a.bin")).expect("remove");
    scratch.write("b.bin", tail_changed(&moved, 0.1));
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
    let moved = |frame: &vigia_core::Frame<'_>| {
        frame.files().iter().any(|change| {
            change.path == "b.bin"
                && change.kind
                    == ChangeKind::Renamed {
                        from: "a.bin".to_owned(),
                    }
        })
    };
    let (mut unlisted, began) = (false, Instant::now());
    while !moved(&frame) {
        assert!(
            began.elapsed() < Duration::from_secs(30),
            "the rename was never listed"
        );
        let now = std::time::SystemTime::now();
        if let Some(wait) = frame.settles_in(now) {
            std::thread::sleep(wait);
            frame
                .advance_if_settled(std::time::SystemTime::now())
                .expect("advance");
        }
        unlisted |= frame.pairs_unlisted();
        pictures.score_pairs(&frame);
        std::thread::sleep(Duration::from_millis(5));
        let found = pictures.found_pairs();
        pictures.pair(&mut frame, found).expect("pair");
        unlisted |= frame.pairs_unlisted();
    }
    assert!(unlisted, "no frame had more pairs than it listed");
}

#[test]
fn full_scores_stop_listing() {
    // More unrelated pairs than the scores hold: the pane stops listing them
    // rather than evict one answer with the next, and goes quiet.
    let scratch = Scratch::new("rename-full-scores");
    for n in 0..70u64 {
        scratch.write(&format!("old/{n:02}.bin"), noise(400 + n, 2_000));
    }
    scratch.commit_all("initial");
    for n in 0..70u64 {
        std::fs::remove_file(scratch.path_of(&format!("old/{n:02}.bin"))).expect("remove");
        scratch.write(&format!("new/{n:02}.bin"), noise(500 + n, 2_000));
    }
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
    let (began, mut quiet, mut walks) = (Instant::now(), 0, 0);
    while quiet < 100 {
        assert!(
            began.elapsed() < Duration::from_secs(10),
            "the pane still walks after 10s, {} walks",
            frame.stats().walks
        );
        let now = std::time::SystemTime::now();
        if let Some(wait) = frame.settles_in(now) {
            std::thread::sleep(wait);
            frame
                .advance_if_settled(std::time::SystemTime::now())
                .expect("advance");
        }
        pictures.score_pairs(&frame);
        std::thread::sleep(Duration::from_millis(2));
        let found = pictures.found_pairs();
        pictures.pair(&mut frame, found).expect("pair");
        quiet = if frame.stats().walks == walks {
            quiet + 1
        } else {
            0
        };
        walks = frame.stats().walks;
    }
    assert_eq!(frame.pair_epoch(), 0, "the scores started again");
}
