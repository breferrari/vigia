//! One entry that cannot be read is one entry, not the frame.

mod support;

use support::{GONE, KEPT, Scratch, index_of};
use vigia_core::Error;

/// How tall the shell draws a file, which is `vigia::view::rows_of`'s rule.
/// Mirrored rather than imported: the shell is not a dependency of this crate.
fn rows_of(_: &vigia_core::FileChange, span: &vigia_core::FileSpan) -> usize {
    if span.unreadable {
        2
    } else {
        1 + span.hunks as usize + span.lines as usize
    }
}

#[test]
fn a_nested_repository_diffs_as_an_empty_addition() {
    let scratch = Scratch::with_nested_repository("core-nested-empty");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");

    let index = index_of(&frame, "nested");
    let (_, diff) = frame
        .diff(index)
        .expect("a nested repository is one entry to skip, not a failed frame");
    assert_eq!(
        (diff.added, diff.removed, diff.hunks.len()),
        (0, 0, 0),
        "git stores no bytes for a directory, so there is nothing to draw under it"
    );
    assert!(
        diff.unreadable.is_none(),
        "a directory is forgiven rather than marked: {:?}",
        diff.unreadable
    );
}

#[test]
fn a_nested_repository_leaves_every_other_file_diffable() {
    let scratch = Scratch::with_nested_repository("core-nested-others");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");

    // Every file, and not just the readable one: the defect is that a single
    // entry the walk cannot read costs every entry beside it.
    for index in 0..frame.files().len() {
        let path = frame.files()[index].path.clone();
        frame
            .diff(index)
            .unwrap_or_else(|e| panic!("{path} did not diff: {e}"));
    }

    let index = index_of(&frame, KEPT);
    let (_, diff) = frame.diff(index).expect("diff");
    assert_eq!(
        (diff.added, diff.removed),
        (1, 0),
        "the edit beside the nested repository is one added line"
    );
}

#[test]
fn one_unreadable_entry_leaves_every_other_file_diffable() {
    let scratch = Scratch::with_a_missing_blob("core-blob-others");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");

    let failed = index_of(&frame, GONE);
    let (_, diff) = frame
        .diff(failed)
        .expect("a failure that names one path stays on that path");
    let reason = diff
        .unreadable
        .clone()
        .expect("the row says why it has no lines");
    assert!(
        !reason.contains(GONE),
        "the heading above the note already carries the path: {reason}"
    );
    assert_eq!(
        (diff.added, diff.removed, diff.hunks.len()),
        (0, 0, 0),
        "a diff that could not be computed reports no lines"
    );

    let kept = index_of(&frame, KEPT);
    let (_, diff) = frame.diff(kept).expect("diff");
    assert_eq!(
        (diff.added, diff.removed),
        (1, 0),
        "the readable file beside it is diffed exactly"
    );
}

#[test]
fn an_unreadable_row_is_re_read_rather_than_served_from_cache() {
    // A **removal**, deliberately, and not the modification the tests above use.
    // `reusable` grants reuse outright to a change with no working-tree side, on
    // an unchanged kind and blob alone, so this is the shape where a failure put
    // in the cache would be served back for the life of the process.
    let scratch = Scratch::new("core-blob-retry");
    scratch.write(GONE, "one\n");
    scratch.commit_all("baseline");
    scratch.point_at_a_missing_blob(GONE);
    std::fs::remove_file(scratch.path_of(GONE)).expect("remove");

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let failed = index_of(&frame, GONE);
    let (change, diff) = frame.diff(failed).expect("first diff");
    assert_eq!(
        change.kind,
        vigia_core::ChangeKind::Removed,
        "the fixture stopped producing the shape this test is about"
    );
    assert!(
        diff.unreadable.is_some(),
        "the fixture's blob is present after all, so nothing here fails"
    );

    // A failed diff describes nothing, so it cannot be evidence about the next
    // tick: the entry is retried until it reads, which is how a transient
    // failure corrects itself with no staleness reasoning anywhere.
    let before = frame.stats().reused;
    frame.advance().expect("advance again");
    let failed = index_of(&frame, GONE);
    frame.diff(failed).expect("second diff");
    assert_eq!(
        frame.stats().reused,
        before,
        "the unreadable row was served from cache instead of being re-read"
    );
}

#[test]
fn the_height_of_an_unreadable_file_matches_what_a_note_draws() {
    let scratch = Scratch::with_a_missing_blob("core-blob-height");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let failed = index_of(&frame, GONE);

    let span = frame
        .rows_of(failed, rows_of)
        .expect("a span is measured without reading anything");
    assert_eq!(
        span, 2,
        "the height path has to know the row carries a note, or it counts a \
         file the screen draws two rows for as one"
    );
}

#[test]
fn a_files_height_recovers_when_the_file_does_and_no_diff_is_asked_for() {
    let scratch = Scratch::with_a_missing_blob("core-blob-heals");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");

    // Reached through `diff` first, because that is the call that produces the
    // contained failure. A failure kept anywhere the height walk trusts is served
    // back to it forever: `fill_span`'s first branch takes a diff already in hand
    // without asking whether it still describes the file.
    let failed = index_of(&frame, GONE);
    assert!(
        frame.diff(failed).expect("diff").1.unreadable.is_some(),
        "the fixture's blob is present after all, so nothing here fails"
    );
    let note = frame.rows_of(failed, rows_of).expect("height");
    assert_eq!(note, 2, "an unreadable file draws a heading and a note");

    // The index entry goes back to the blob HEAD holds, so the file reads again.
    // Written directly rather than with `git reset`, which reads the entry it is
    // replacing and so cannot replace one that names a blob nothing holds.
    let real = scratch.git(&["rev-parse", &format!("HEAD:{GONE}")]);
    scratch.git(&[
        "update-index",
        "--cacheinfo",
        &format!("100644,{},{GONE}", real.trim()),
    ]);
    frame.advance().expect("advance");
    let healed = index_of(&frame, GONE);
    let rows = frame.rows_of(healed, rows_of).expect("height");

    // Against a frame that never saw the failure, rather than against a number:
    // what this file is worth is the diff's business, and the claim here is only
    // that carrying a failure through does not change it.
    let mut fresh = worktree.frame();
    fresh.advance().expect("advance");
    let oracle = fresh
        .rows_of(index_of(&fresh, GONE), rows_of)
        .expect("height");
    assert!(
        oracle > 2,
        "the oracle draws a note too, so this would pass on a frame that never \
         recovered"
    );
    assert_eq!(
        rows, oracle,
        "the height stayed at the note after the file became readable, so a \
         reader who never scrolls to it keeps a scrollbar scaled to a diff \
         nobody draws"
    );
}

#[test]
fn a_file_that_fails_after_it_diffed_does_not_keep_the_height_it_had() {
    let scratch = Scratch::new("core-blob-then-fails");
    scratch.write(GONE, "one\n");
    scratch.commit_all("baseline");
    scratch.write(GONE, "one\ntwo\n");

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let index = index_of(&frame, GONE);
    frame.diff(index).expect("the file reads on the first tick");
    let whole = frame.rows_of(index, rows_of).expect("height");
    assert!(
        whole > 2,
        "the fixture draws a note before it is broken, so nothing here is tested"
    );

    // `fill_span` takes a diff in hand without revalidating it, so a diff left in
    // the cache by the tick that succeeded is a height nothing draws.
    scratch.point_at_a_missing_blob(GONE);
    frame.advance().expect("advance");
    let index = index_of(&frame, GONE);
    assert!(
        frame.diff(index).expect("contained").1.unreadable.is_some(),
        "the fixture stopped breaking the file this test is about"
    );
    let now = frame.rows_of(index, rows_of).expect("height");
    assert_eq!(
        now, 2,
        "the height still describes the diff from before the file stopped \
         reading, so the scrollbar is scaled to rows the pane does not draw"
    );
}

/// A fifo is one of the four things `gix` calls untrackable, and the only class
/// where reading the entry does not fail but blocks: `open` on a pipe with no
/// writer waits for one, on the thread the pane draws from.
#[cfg(unix)]
#[test]
fn a_fifo_in_the_worktree_does_not_hang_the_frame() {
    use std::sync::mpsc;
    use std::time::Duration;

    let scratch = Scratch::new("core-fifo");
    scratch.write(KEPT, "one\n");
    scratch.commit_all("baseline");
    scratch.write(KEPT, "one\ntwo\n");
    let made = std::process::Command::new("mkfifo")
        .arg(scratch.path_of("pipe"))
        .status()
        .is_ok_and(|status| status.success());
    assert!(
        made,
        "mkfifo is not available, so this gate asserts nothing"
    );

    let root = scratch.root().to_path_buf();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let worktree = vigia_core::Worktree::discover(&root).expect("discover");
        let mut frame = worktree.frame();
        frame.advance().expect("advance");
        let drawn: Vec<bool> = (0..frame.files().len())
            .map(|index| frame.diff(index).is_ok())
            .collect();
        let _ = tx.send(drawn);
    });

    // A bound rather than an assertion about speed: the failure this names is
    // unbounded, so any bound tells it from a pass. Detached on purpose, since a
    // thread parked in `open` never returns and the harness exits over it.
    let drawn = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the frame is still inside a read of the pipe");
    assert!(
        drawn.iter().all(|ok| *ok),
        "a fifo is an entry to skip, not a failed frame: {drawn:?}"
    );
}

#[test]
fn a_failure_that_is_not_one_files_still_ends_the_frame() {
    let io = || std::io::Error::other("probe");
    let one_file: [Error; 3] = [
        Error::Read {
            path: "a.txt".to_owned(),
            source: io(),
        },
        Error::MissingBlob {
            path: "a.txt".to_owned(),
        },
        Error::Filter {
            path: "a.txt".to_owned(),
            source: Box::new(io()),
        },
    ];
    for error in one_file {
        assert!(
            error.of_one_file().is_some(),
            "{error} names one path, so the frame contains it"
        );
    }

    let whole_frame: [Error; 4] = [
        Error::Status(Box::new(io())),
        Error::Watch(Box::new(io())),
        Error::FilterSetup(Box::new(io())),
        Error::Bare,
    ];
    for error in whole_frame {
        assert!(
            error.of_one_file().is_none(),
            "{error} is not one file's failure, and containing it would draw a \
             frame nothing vouches for"
        );
    }
}
