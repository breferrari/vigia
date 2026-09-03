//! I5 over a tree the pane cannot read whole: the frame still advances.
//!
//! The invariant is *correct with zero interaction*, and its own gate cannot see
//! this class, because a scripted edit sequence over a readable fixture never
//! meets an entry that fails.

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

use vigia::{App, Body, Row, View, diff_rows};
use vigia_core::{Frame, Highlighter, History};

use support::{KEPT, Scratch};

/// Tall enough that nothing is scrolled off, so the whole diff is asserted.
const ALL_ROWS: usize = 500;

/// One frame, the way the shell builds one.
fn collect(frame: &mut Frame, app: &mut App) -> View {
    let mut highlighter = Highlighter::eager();
    let history = History::new();
    app.view(frame, &mut highlighter, &history, Body::diff_only(ALL_ROWS))
        .expect("one entry that cannot be read must not end the frame")
}

/// Every row the diff region drew, as the text it carries.
fn drawn(view: &View) -> Vec<String> {
    view.rows
        .iter()
        .map(|row| match row {
            Row::File(entry) => entry.path.clone(),
            Row::Line { text, .. } | Row::Wrap { text, .. } => text.clone(),
            Row::Note(note) => note.clone(),
            Row::Hunk { .. } => "@@".to_owned(),
            Row::Gap => String::new(),
        })
        .collect()
}

#[test]
fn a_first_frame_over_a_nested_repository_is_not_empty() {
    let scratch = Scratch::with_nested_repository("shell-nested-first");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let view = collect(&mut frame, &mut App::new());

    // The reported startup symptom: with no previous frame to hold, the shell
    // drew `View::default()`, which reads `no staged or unstaged changes` over a
    // tree full of them.
    assert!(
        view.files > 0,
        "the first frame claimed an empty worktree over {} changed files",
        frame.files().len()
    );
    assert!(
        drawn(&view).iter().any(|row| row.contains(KEPT)),
        "the file beside the nested repository was not drawn: {:?}",
        drawn(&view)
    );
}

#[test]
fn a_nested_repository_does_not_stop_the_pane_from_advancing() {
    let scratch = Scratch::with_nested_repository("shell-nested-advance");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    let mut app = App::new();
    frame.advance().expect("advance");
    let before = drawn(&collect(&mut frame, &mut app));
    assert!(
        !before.iter().any(|row| row.contains("three")),
        "the fixture already held the edit this test makes: {before:?}"
    );

    // The reported mid-session symptom: every edit after the nested repository
    // appeared was invisible, because the frame was held at the last one that
    // had read whole.
    scratch.write(KEPT, "one\ntwo\nthree\n");
    frame.advance().expect("advance after the edit");
    let after = drawn(&collect(&mut frame, &mut app));
    assert!(
        after.iter().any(|row| row.contains("three")),
        "the edit after the nested repository appeared never reached the pane: \
         {after:?}"
    );
}

#[test]
fn an_unreadable_file_draws_a_note_and_the_rest_of_the_diff() {
    let scratch = Scratch::with_a_missing_blob("shell-blob-note");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let view = collect(&mut frame, &mut App::new());

    let notes: Vec<&String> = view
        .rows
        .iter()
        .filter_map(|row| match row {
            Row::Note(note) => Some(note),
            _ => None,
        })
        .collect();
    assert_eq!(
        notes.len(),
        1,
        "one row could not be read, so one note says why: {:?}",
        drawn(&view)
    );
    assert!(
        !notes[0].is_empty(),
        "a note that says nothing is a row a reader cannot act on"
    );

    assert!(
        drawn(&view).iter().any(|row| row.contains("two")),
        "the readable file's own lines went with the unreadable one: {:?}",
        drawn(&view)
    );
}

#[test]
fn the_height_and_the_drawn_rows_agree_over_an_unreadable_file() {
    let scratch = Scratch::with_a_missing_blob("shell-blob-height");
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    frame.advance().expect("advance");
    let view = collect(&mut frame, &mut App::new());

    // Two walks over the same frame, and only one of them builds any text. A
    // disagreement here is a scrollbar scaled to a diff nobody drew.
    let counted = diff_rows(&mut frame).expect("height");
    assert_eq!(
        counted,
        view.rows.len(),
        "the counted height and the drawn rows disagree: {:?}",
        drawn(&view)
    );
}
