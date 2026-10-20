//! A rename the frame could not afford to score, scored on the worker and
//! paired on a later frame. `SPEC.md` §11.1.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use std::time::{Duration, Instant};

use ratatui_image::picker::Picker;
use support::{Scratch, changes_sorted};
use vigia::pictures::{Inks, Pictures};
use vigia_core::ChangeKind;

/// `length` bytes of noise from `seed`, a NUL among them.
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

#[test]
fn binary_rename_paired_by_worker() {
    let old = noise(1, 2 * 1024 * 1024);
    let mut new = old.clone();
    let tail = new.len() - new.len() / 10;
    for byte in &mut new[tail..] {
        *byte ^= 0x5a;
    }
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
    for (key, score) in found {
        frame.learn_pair(key, score);
    }
    frame.advance().expect("advance");
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
