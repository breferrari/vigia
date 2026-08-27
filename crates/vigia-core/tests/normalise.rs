//! Does the diff normalise the working-tree side the way git does?

mod support;

use support::{Numstat, Scratch, changes_sorted, delta, numbered_lines};
use vigia_core::{Error, Frame, Worktree};

/// The reported case: a CRLF worktree file over an LF blob, and no real edit.
#[test]
fn a_crlf_worktree_file_whose_blob_is_lf_diffs_as_no_change() {
    let scratch = Scratch::crlf_worktree("normalise-clean", Some("* text=auto eol=lf\n"));
    scratch.write("a.txt", numbered_lines(20));
    scratch.commit_all("initial");

    // What an editor on Windows does to a file the attributes say is LF.
    scratch.write_crlf("a.txt", &numbered_lines(20));

    assert_eq!(
        scratch.git_numstat("a.txt"),
        Numstat::Unchanged,
        "the fixture is wrong: git has to call this file unchanged, or there is \
         nothing here to disagree with git about"
    );

    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);
    let change = changes
        .iter()
        .find(|c| c.path == "a.txt")
        .expect("status lists the file, exactly as `git status` does");

    let diff = worktree.diff(change).expect("diff");
    assert_eq!(
        (diff.added, diff.removed),
        (0, 0),
        "git reports no change and this reported +{} −{}, so a file that changed \
         nothing draws as a rewrite",
        diff.added,
        diff.removed
    );
    assert!(diff.hunks.is_empty(), "no change means no hunks to draw");
}

/// The commoner and worse case: the plain installed default, no `.gitattributes`
/// anywhere, where a checkout puts CRLF on disk against an LF blob.
#[test]
fn a_one_line_edit_inside_a_crlf_file_diffs_as_one_line() {
    let scratch = Scratch::crlf_worktree("normalise-edit", None);
    scratch.write("a.txt", numbered_lines(20));
    scratch.commit_all("initial");
    scratch.checkout("a.txt");

    let on_disk = std::fs::read(scratch.path_of("a.txt")).expect("read");
    assert!(
        on_disk.windows(2).any(|w| w == b"\r\n"),
        "the fixture is wrong: a checkout under autocrlf=true has to leave CRLF \
         on disk, or the filter has nothing to do"
    );

    // One line, edited in place, in the terminator the file already uses.
    scratch.write_crlf(
        "a.txt",
        &numbered_lines(20).replace("line 10\n", "CHANGED\n"),
    );

    assert_eq!(
        scratch.git_numstat("a.txt"),
        Numstat::Lines(1, 1),
        "the oracle disagrees with the fixture's own description of itself"
    );

    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);
    let diff = worktree.diff(&changes[0]).expect("diff");

    assert_eq!(
        (diff.added, diff.removed),
        (1, 1),
        "git reports one line either way and this reported +{} −{}",
        diff.added,
        diff.removed
    );
    assert_eq!(diff.hunks.len(), 1, "one edit is one hunk");
}

/// The oracle at full strength: hunk boundaries, not just totals.
#[test]
fn a_normalised_diff_matches_git_hunk_for_hunk() {
    const TOTAL_LINES: usize = 60;
    const FIRST_EDIT: u32 = 20;

    for gap in 0..=10u32 {
        let scratch = Scratch::crlf_worktree(&format!("normalise-gap-{gap}"), None);
        scratch.write("a.txt", numbered_lines(TOTAL_LINES));
        scratch.commit_all("initial");
        scratch.checkout("a.txt");

        let second_edit = FIRST_EDIT + gap + 1;
        let edited = numbered_lines(TOTAL_LINES)
            .replace(&format!("line {FIRST_EDIT}\n"), "FIRST\n")
            .replace(&format!("line {second_edit}\n"), "SECOND\n");
        scratch.write_crlf("a.txt", &edited);

        let worktree = scratch.worktree();
        let changes = changes_sorted(&worktree);
        let diff = worktree.diff(&changes[0]).expect("diff");

        let ours: Vec<(u32, u32, u32, u32)> = diff
            .hunks
            .iter()
            .map(|h| (h.old_start, h.old_lines, h.new_start, h.new_lines))
            .collect();

        assert_eq!(
            ours,
            scratch.git_hunk_headers("a.txt"),
            "disagreed with git over a CRLF worktree when {gap} unchanged lines \
             separate the edits"
        );
    }
}

/// Normalisation follows the attributes rather than the platform.
#[test]
fn the_binary_attribute_turns_normalisation_off() {
    let changed = |attributes: Option<&str>, name: &str| -> (u32, u32) {
        let scratch = Scratch::crlf_worktree(name, attributes);
        scratch.write("a.txt", numbered_lines(20));
        scratch.commit_all("initial");
        scratch.write_crlf("a.txt", &numbered_lines(20));

        let worktree = scratch.worktree();
        let changes = changes_sorted(&worktree);
        let change = changes
            .iter()
            .find(|c| c.path == "a.txt")
            .expect("the rewritten file is changed");
        let diff = worktree.diff(change).expect("diff");
        (diff.added, diff.removed)
    };

    assert_eq!(
        changed(None, "normalise-attr-off"),
        (0, 0),
        "line endings alone are not a change when git would normalise them"
    );
    assert_eq!(
        changed(Some("a.txt binary\n"), "normalise-attr-binary"),
        (20, 20),
        "`binary` implies `-text`, so git converts nothing and every line really \
         does differ; reporting no change here would mean the attributes are \
         being ignored"
    );
}

/// A `.gitattributes` written mid-session reaches the very next frame.
#[test]
fn attributes_written_mid_session_reach_the_next_frame() {
    let scratch = Scratch::crlf_worktree("normalise-restat", None);
    scratch.write("a.txt", numbered_lines(20));
    scratch.commit_all("initial");
    scratch.checkout("a.txt");
    scratch.write_crlf(
        "a.txt",
        &numbered_lines(20).replace("line 10\n", "CHANGED\n"),
    );

    let diff_of = |worktree: &Worktree| -> (u32, u32) {
        let mut frame = worktree.frame();
        frame.advance().expect("advance");
        let at = frame
            .files()
            .iter()
            .position(|c| c.path == "a.txt")
            .expect("a.txt is changed");
        let (_, diff) = frame.diff(at).expect("diff");
        (diff.added, diff.removed)
    };

    let worktree = scratch.worktree();
    assert_eq!(
        diff_of(&worktree),
        (1, 1),
        "the fixture is wrong: the edit has to normalise to one line before the \
         attributes change, or there is no stale answer to catch"
    );

    // The agent in the other pane marks the file binary, so `-text` applies and
    // the CRLF difference stops being normalised away.
    scratch.write(".gitattributes", "a.txt binary\n");

    let restarted = diff_of(&scratch.worktree());
    assert_eq!(
        restarted,
        (20, 20),
        "the control is wrong: a restart has to see the new attributes, or this \
         test cannot tell a stale filter from a correct one"
    );
    assert_eq!(
        diff_of(&worktree),
        restarted,
        "the running session drew +{} −{} where a restart draws +{} −{}, so the \
         filter is answering from `.gitattributes` that no longer exists",
        diff_of(&worktree).0,
        diff_of(&worktree).1,
        restarted.0,
        restarted.1
    );
}

#[test]
fn a_running_frame_drops_what_it_cached_when_attributes_change() {
    // The gate above cannot see a cache, and that is why this one exists.
    let scratch = Scratch::crlf_worktree("normalise-carried", None);
    scratch.write("a.txt", numbered_lines(20));
    scratch.commit_all("initial");
    scratch.checkout("a.txt");
    scratch.write_crlf(
        "a.txt",
        &numbered_lines(20).replace("line 10\n", "CHANGED\n"),
    );

    let worktree = scratch.worktree();
    let mut frame = worktree.frame();

    // One frame, held. Settle it so the artefacts are provable, and prove they
    // are being carried rather than recomputed: without this the assertions
    // below pass against a frame that caches nothing at all.
    let rows = |frame: &mut Frame| frame.height(|_, span| span.lines as usize).expect("height");
    let primed = support::settle_spans(&mut frame);
    assert_eq!(primed, 1, "the fixture is not one changed file");
    let carried = rows(&mut frame);

    // And the diff cache is populated too, before anything changes.
    let at = frame
        .files()
        .iter()
        .position(|c| c.path == "a.txt")
        .expect("a.txt is changed");
    let (_, diff) = frame.diff(at).expect("diff");
    let stale = (diff.added, diff.removed);

    let before = frame.stats();
    frame.advance().expect("advance");
    let idle = rows(&mut frame);
    let (_, diff) = frame.diff(at).expect("diff");
    let idle_diff = (diff.added, diff.removed);
    let cost = delta(before, frame.stats());
    assert_eq!(
        cost.measured, 0,
        "an idle tick re-measured, so no span is carried and this test cannot \
         tell a stale height from a fresh one"
    );
    assert_eq!(
        cost.computed, 0,
        "an idle tick recomputed the diff, so none is carried either and the \
         diff half below cannot tell a dropped cache from a kept one"
    );
    assert_eq!(idle, carried, "an idle tick changed the height");
    assert_eq!(idle_diff, stale, "an idle tick changed the diff");

    // The agent in the other pane marks the file binary. `a.txt` is untouched:
    // same length, same modification time, same index blob.
    scratch.write(".gitattributes", "a.txt binary\n");

    let truth = {
        let mut cold = worktree.frame();
        cold.advance().expect("advance");
        rows(&mut cold)
    };
    assert_ne!(
        truth, carried,
        "the control is wrong: the attributes change has to move the height, or \
         this test cannot tell a dropped cache from a kept one"
    );

    frame.advance().expect("advance");
    assert_eq!(
        rows(&mut frame),
        truth,
        "the running frame reports a height computed under attributes that no \
         longer apply, and nothing will correct it until that file is touched"
    );

    // And the diff cache with it, which is the same hole one artefact over. The
    // diff asked for here was computed and cached before the attributes moved,
    // so a frame that kept it answers with the stale pair.
    let at = frame
        .files()
        .iter()
        .position(|c| c.path == "a.txt")
        .expect("a.txt is still changed");
    let (_, diff) = frame.diff(at).expect("diff");
    let (added, removed) = (diff.added, diff.removed);
    let mut cold = worktree.frame();
    cold.advance().expect("advance");
    let (_, fresh) = cold.diff(at).expect("diff");
    let truth_diff = (fresh.added, fresh.removed);
    assert_ne!(
        truth_diff, stale,
        "the control is wrong: the attributes change has to move the diff too, \
         or the assertion below cannot tell a dropped cache from a kept one"
    );
    assert_eq!(
        (added, removed),
        truth_diff,
        "the running frame drew +{added} −{removed} where a restart draws +{} \
         −{}, so a diff computed under the old attributes survived them",
        fresh.added,
        fresh.removed
    );
}

/// A configured external clean driver is not executed.
#[test]
fn an_external_clean_driver_is_never_run() {
    let scratch = Scratch::crlf_worktree("normalise-driver", None);
    scratch.write("a.txt", numbered_lines(20));
    scratch.commit_all("initial");
    scratch.checkout("a.txt");

    scratch.git(&["config", "filter.shout.clean", "sed s/line/SHOUT/"]);
    scratch.git(&["config", "filter.shout.required", "true"]);
    scratch.write(".gitattributes", "a.txt filter=shout\n");
    scratch.write_crlf(
        "a.txt",
        &numbered_lines(20).replace("line 10\n", "CHANGED\n"),
    );

    assert_eq!(
        scratch.git_numstat("a.txt"),
        Numstat::Lines(20, 20),
        "the fixture is wrong: git has to run the driver and see every line \
         change, or there is no spawning to decline"
    );

    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);
    let a = changes
        .iter()
        .find(|c| c.path == "a.txt")
        .expect("a.txt is changed");
    let diff = worktree
        .diff(a)
        .expect("a required driver we decline to run must not fail the frame");

    assert_eq!(
        (diff.added, diff.removed),
        (1, 1),
        "reported +{} −{}. 20/20 would mean the driver ran, which is a process \
         per file per frame and is what §6 forbids",
        diff.added,
        diff.removed
    );
}

/// `core.safecrlf` does not reach the diff, because it guards writing.
#[test]
fn safecrlf_does_not_fail_a_frame() {
    let scratch = Scratch::crlf_worktree("normalise-safecrlf", None);
    scratch.write("a.txt", numbered_lines(20));
    scratch.commit_all("initial");
    scratch.checkout("a.txt");
    // After the baseline commit on purpose: under `safecrlf=true` even `git add`
    // refuses this content, so the fixture cannot be built with it set earlier.
    scratch.git(&["config", "core.safecrlf", "true"]);

    let mixed = numbered_lines(20)
        .replace('\n', "\r\n")
        .replace("line 7\r\n", "line 7\n");
    std::fs::write(scratch.path_of("a.txt"), mixed).expect("write the mixed fixture");

    assert_eq!(
        scratch.git_numstat("a.txt"),
        Numstat::Unchanged,
        "the fixture is wrong: git has to diff this file happily, or there is no \
         disagreement to be had about whether we may refuse it"
    );

    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);
    let change = changes
        .iter()
        .find(|c| c.path == "a.txt")
        .expect("a.txt is reported by status");

    let diff = worktree
        .diff(change)
        .expect("a round-trip guard on writing must not fail a frame that reads");
    assert_eq!(
        (diff.added, diff.removed),
        (0, 0),
        "reported +{} −{} where git reports no change",
        diff.added,
        diff.removed
    );
}

/// A path whose attributes name something unhonourable reports that path.
#[test]
fn an_unhonourable_attribute_names_the_file_it_came_from() {
    let scratch = Scratch::crlf_worktree("normalise-bad-encoding", None);
    scratch.write("a.txt", numbered_lines(20));
    scratch.commit_all("initial");
    scratch.write(
        ".gitattributes",
        "a.txt working-tree-encoding=NOT-AN-ENCODING\n",
    );
    scratch.write_crlf("a.txt", &numbered_lines(20).replace("line 3\n", "X\n"));

    let worktree = scratch.worktree();
    let changes = changes_sorted(&worktree);
    let change = changes
        .iter()
        .find(|c| c.path == "a.txt")
        .expect("a.txt is changed");

    let error = worktree
        .diff(change)
        .expect_err("an encoding that does not exist produced a diff anyway");
    match error {
        Error::Filter { path, .. } => assert_eq!(
            path, "a.txt",
            "the failure has to name the file, since it is one path's and not \
             the repository's"
        ),
        other => panic!("reported {other:?}, which is not one path's failure"),
    }
}

/// A filter that cannot be assembled fails the diff rather than falling back.
#[test]
fn a_filter_that_cannot_be_built_fails_the_diff() {
    let scratch = Scratch::crlf_worktree("normalise-broken-index", None);
    scratch.write("a.txt", numbered_lines(20));
    scratch.commit_all("initial");
    scratch.checkout("a.txt");
    scratch.write_crlf("a.txt", &numbered_lines(20).replace("line 3\n", "X\n"));

    let worktree = scratch.worktree();
    // Enumerated while the index is still good, so nothing has built a filter.
    let changes = changes_sorted(&worktree);
    let change = changes
        .iter()
        .find(|c| c.path == "a.txt")
        .expect("a.txt is changed")
        .clone();

    std::fs::write(scratch.path_of(".git/index"), vec![0xABu8; 128]).expect("corrupt the index");

    let error = worktree
        .diff(&change)
        .expect_err("an unbuildable filter was reported as a successful diff");
    assert!(
        matches!(error, Error::FilterSetup(_)),
        "reported {error:?}, so a filter that could not be built either fell back \
         to the raw bytes or was mistaken for some other failure"
    );
}

/// Normalising costs no extra read and no extra `stat`.
#[test]
fn normalising_costs_no_extra_read_or_probe() {
    const FILES: usize = 8;
    const LINES: usize = 200;

    let cost = |scratch: &Scratch, crlf: bool| {
        for f in 0..FILES {
            scratch.write(&format!("src/mod_{f}.rs"), numbered_lines(LINES));
        }
        scratch.commit_all("baseline");
        for f in 0..FILES {
            let path = format!("src/mod_{f}.rs");
            if crlf {
                scratch.checkout(&path);
            }
            let edited = numbered_lines(LINES).replace("line 100\n", "CHANGED\n");
            if crlf {
                scratch.write_crlf(&path, &edited);
            } else {
                scratch.write(&path, &edited);
            }
        }

        let worktree = scratch.worktree();
        let mut frame = worktree.frame();
        support::materialise(&mut frame);
        assert_eq!(
            frame.files().len(),
            FILES,
            "the fixture is not {FILES} files"
        );
        frame.stats()
    };

    let lf = cost(&Scratch::new("normalise-cost-lf"), false);
    let crlf = cost(&Scratch::crlf_worktree("normalise-cost-crlf", None), true);

    assert!(
        lf.bytes > 0,
        "a cold frame compared nothing, so nothing was measured"
    );
    assert_eq!(
        lf.computed, crlf.computed,
        "the two fixtures did not do the same work, so their costs are not \
         comparable"
    );
    assert_eq!(
        crlf.bytes,
        lf.bytes,
        "a CRLF worktree compared {} bytes against {} for its LF twin, a \
         difference of {}, so the working-tree side reached the diff unnormalised",
        crlf.bytes,
        lf.bytes,
        crlf.bytes as i64 - lf.bytes as i64
    );
    assert_eq!(
        crlf.probes, lf.probes,
        "normalising took {} probes against {} without it, so it is stat-ing the \
         file it was handed",
        crlf.probes, lf.probes
    );
}
