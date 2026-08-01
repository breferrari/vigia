//! Does the diff normalise the working-tree side the way git does?
//!
//! Git compares the working tree against the index *through* its clean filter,
//! so on a checkout with `core.autocrlf=true` the CRLF on disk is turned back
//! into the LF the blob holds before anything is compared. Reading the raw bytes
//! instead makes every line of every such file differ from its stored form.
//!
//! That was [#65](https://github.com/breferrari/vigia/issues/65), and it was
//! reported from a real screen: a 21-line edit drawn as **+905 −885**, on a file
//! `git diff` called unchanged, on the installed default of a tier-1 target.
//!
//! **`git` is the oracle in every test here**, as it is in `fidelity.rs`, and for
//! a sharper reason: the expected numbers below are exactly the ones a
//! hand-written expectation would have got wrong, since the whole defect is a
//! disagreement with git about what "changed" means.

mod support;

use support::Scratch;
use vigia_core::{FileChange, Worktree};

/// All changes, sorted by path so assertions do not depend on walk order.
fn changes_of(worktree: &Worktree) -> Vec<FileChange> {
    let mut all: Vec<FileChange> = worktree
        .changes()
        .expect("enumerate changes")
        .map(|c| c.expect("change"))
        .collect();
    all.sort_by(|a, b| a.path.cmp(&b.path));
    all
}

fn numbered_lines(count: usize) -> String {
    (1..=count).map(|i| format!("line {i}\n")).collect()
}

/// What `git diff --numstat` says about one path: `(added, removed)`.
///
/// `None` when git reports the file as unchanged, which is the answer the first
/// test below is about, and `Some((0, 0))` would not be the same claim.
fn git_numstat(scratch: &Scratch, rela: &str) -> Option<(u32, u32)> {
    let out = scratch.git(&["diff", "--numstat", "--", rela]);
    let line = out.lines().next()?;
    let mut fields = line.split('\t');
    let added = fields.next()?.parse().ok()?;
    let removed = fields.next()?.parse().ok()?;
    Some((added, removed))
}

/// Put the file on disk the way `git checkout` would, rather than the way the
/// test wrote it.
///
/// The difference is the entire fixture for the no-attributes case: with
/// `core.autocrlf=true` a checkout writes CRLF even though the test's own source
/// text is LF, so going through git is what produces the state a Windows reader
/// actually has. Writing the CRLF by hand would test the same bytes for a reason
/// that could stop being true.
fn checkout(scratch: &Scratch, rela: &str) {
    std::fs::remove_file(scratch.path_of(rela)).expect("remove before checkout");
    scratch.git(&["checkout", "--", rela]);
}

/// The reported case: a CRLF worktree file over an LF blob, and no real edit.
///
/// `git diff` reports nothing at all here, so anything this draws as changed is
/// a line the reader has to scroll past to reach a change that exists.
///
/// The file is still **listed**, and that is the ruling rather than a leftover:
/// dropping it from the list would need its content during `Frame::advance`,
/// which I4 forbids, and a header count disagreeing with the body is exactly the
/// contradiction B3's empty state exists to remove. `git status` lists it too.
/// See `SPEC.md` §11.1.
#[test]
fn a_crlf_worktree_file_whose_blob_is_lf_diffs_as_no_change() {
    let scratch = Scratch::crlf_worktree("normalise-clean", Some("* text=auto eol=lf\n"));
    scratch.write("a.txt", numbered_lines(20));
    scratch.commit_all("initial");

    // What an editor on Windows does to a file the attributes say is LF.
    scratch.write_crlf("a.txt", &numbered_lines(20));

    assert_eq!(
        git_numstat(&scratch, "a.txt"),
        None,
        "the fixture is wrong: git has to call this file unchanged, or there is \
         nothing here to disagree with git about"
    );

    let worktree = scratch.worktree();
    let changes = changes_of(&worktree);
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
///
/// Every text file in every repository is in this state on a default Windows
/// install, so without normalisation **every** edit to **any** of them reads as
/// a full rewrite. This was not in the issue report and is the larger half of it.
#[test]
fn a_one_line_edit_inside_a_crlf_file_diffs_as_one_line() {
    let scratch = Scratch::crlf_worktree("normalise-edit", None);
    scratch.write("a.txt", numbered_lines(20));
    scratch.commit_all("initial");
    checkout(&scratch, "a.txt");

    let on_disk = std::fs::read(scratch.path_of("a.txt")).expect("read");
    assert!(
        on_disk.windows(2).any(|w| w == b"\r\n"),
        "the fixture is wrong: a checkout under autocrlf=true has to leave CRLF \
         on disk, or the filter has nothing to do"
    );

    // One line, edited in place, in the terminator the file already uses.
    scratch.write_crlf("a.txt", &numbered_lines(20).replace("line 10\n", "CHANGED\n"));

    assert_eq!(
        git_numstat(&scratch, "a.txt"),
        Some((1, 1)),
        "the oracle disagrees with the fixture's own description of itself"
    );

    let worktree = scratch.worktree();
    let changes = changes_of(&worktree);
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
///
/// The same sweep `fidelity.rs::hunk_grouping_boundary_matches_git_at_every_gap`
/// runs, one configuration axis over. A totals-only check passes for a diff that
/// agrees on how much changed and disagrees on where, and where is what the pane
/// draws.
#[test]
fn a_normalised_diff_matches_git_hunk_for_hunk() {
    const TOTAL_LINES: usize = 60;
    const FIRST_EDIT: u32 = 20;

    for gap in 0..=10u32 {
        let scratch = Scratch::crlf_worktree(&format!("normalise-gap-{gap}"), None);
        scratch.write("a.txt", numbered_lines(TOTAL_LINES));
        scratch.commit_all("initial");
        checkout(&scratch, "a.txt");

        let second_edit = FIRST_EDIT + gap + 1;
        let edited = numbered_lines(TOTAL_LINES)
            .replace(&format!("line {FIRST_EDIT}\n"), "FIRST\n")
            .replace(&format!("line {second_edit}\n"), "SECOND\n");
        scratch.write_crlf("a.txt", &edited);

        let worktree = scratch.worktree();
        let changes = changes_of(&worktree);
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
///
/// Two fixtures differing in one term, which is the shape `SPEC.md` §7 asks for:
/// the same bytes, the same configuration, and one `.gitattributes` line
/// between them. `binary` implies `-text`, so git converts nothing and the CRLF
/// difference is real; without it the same file normalises to nothing at all.
/// Asserting only the first would pass against a filter that never ran.
///
/// What this does **not** claim is that a `binary`-marked file is drawn the way
/// git draws it. `git diff` reports `Binary files differ` and this still diffs it
/// as text, because `binary` also implies `-diff` and nothing here honours that:
/// [#66](https://github.com/breferrari/vigia/issues/66).
#[test]
fn the_binary_attribute_turns_normalisation_off() {
    let changed = |attributes: Option<&str>, name: &str| -> (u32, u32) {
        let scratch = Scratch::crlf_worktree(name, attributes);
        scratch.write("a.txt", numbered_lines(20));
        scratch.commit_all("initial");
        scratch.write_crlf("a.txt", &numbered_lines(20));

        let worktree = scratch.worktree();
        let changes = changes_of(&worktree);
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

/// Normalising costs no extra read and no extra `stat`.
///
/// The structural half, and exact rather than a threshold. A CRLF worktree and
/// its LF twin hold **identical normalised content**, so a frame over one must
/// compare the same number of bytes and take the same number of probes as a
/// frame over the other. Two fixtures differing in one term, per `SPEC.md` §7.
///
/// Byte equality is the stronger of the two claims and the one that would catch
/// a regression: it holds only if the filter converted, since the raw CRLF file
/// is a carriage return per line larger. A filter that read the file a second
/// time to convert it would move `probes` instead.
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
                checkout(scratch, &path);
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
        assert_eq!(frame.files().len(), FILES, "the fixture is not {FILES} files");
        frame.stats()
    };

    let lf = cost(&Scratch::new("normalise-cost-lf"), false);
    let crlf = cost(&Scratch::crlf_worktree("normalise-cost-crlf", None), true);

    assert!(lf.bytes > 0, "a cold frame compared nothing, so nothing was measured");
    assert_eq!(
        lf.computed, crlf.computed,
        "the two fixtures did not do the same work, so their costs are not \
         comparable"
    );
    assert_eq!(
        crlf.bytes, lf.bytes,
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
