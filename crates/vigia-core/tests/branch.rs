//! What the empty state calls the branch.

mod support;

use support::Scratch;

#[test]
fn the_branch_is_the_short_name_of_what_head_points_at() {
    // Shortened, because `refs/heads/` is eleven columns of prefix that says
    // nothing: every branch has it. A slash *inside* the name survives, which is
    // why the fixture uses one rather than a bare word.
    let scratch = Scratch::new("branch-named");
    scratch.write("a.txt", "one\n");
    scratch.commit_all("first");
    scratch.git(&["checkout", "-q", "-b", "feature/glance"]);

    assert_eq!(
        scratch.worktree().branch(),
        Some("feature/glance".to_owned())
    );
}

#[test]
fn a_detached_head_names_no_branch() {
    // Ordinary rather than exceptional: a rebase or a bisect leaves an agent
    // here routinely. `SPEC.md` §11.1 rules that the line drops the branch
    // instead of inventing one, because `HEAD@abc123` would put a commit id in a
    // monitor that shows no commits.
    let scratch = Scratch::new("branch-detached");
    scratch.write("a.txt", "one\n");
    scratch.commit_all("first");
    scratch.git(&["checkout", "-q", "--detach"]);

    assert_eq!(scratch.worktree().branch(), None);
}

#[test]
fn a_repository_with_no_commits_still_names_its_branch() {
    // The first impression B3 is about: `vigia .` in a tree an agent has not
    // committed to yet. HEAD is a symbolic ref to a branch that does not exist,
    // and the honest answer is the branch it *will* be, not nothing.
    let scratch = Scratch::new("branch-unborn");
    scratch.git(&["symbolic-ref", "HEAD", "refs/heads/trunk"]);

    assert_eq!(scratch.worktree().branch(), Some("trunk".to_owned()));
}
