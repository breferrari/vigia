//! What the empty state calls the branch.
//!
//! `SPEC.md` §11.1 rules B3: a worktree with nothing in it names the branch it is
//! sitting on, because two agents on two worktrees of one repository are
//! otherwise identical on screen. The branch is **orientation**, not the
//! comparison: this diff is the working tree against the index and HEAD does not
//! enter into it, which is why the answer may legitimately be nothing at all.
//!
//! Three states, and the third is the one worth having a fixture for. A
//! repository with no commits is what an agent's very first minute looks like,
//! and it is exactly the first impression B3 exists to specify.

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
    //
    // Pointed somewhere other than the default deliberately. `Scratch::new`
    // inits with `-b main`, so asserting `main` would also pass against an
    // implementation that returned a hardcoded default and never read HEAD.
    let scratch = Scratch::new("branch-unborn");
    scratch.git(&["symbolic-ref", "HEAD", "refs/heads/trunk"]);

    assert_eq!(scratch.worktree().branch(), Some("trunk".to_owned()));
}
