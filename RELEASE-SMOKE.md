# Release smoke — run against the built artifact, before the release is dispatched

CI green is necessary and not sufficient. A sibling project shipped two
consecutive patches with a green matrix that broke the flagship install on day
one, and its fix was this checklist's ancestor.

**Dispatching `bump and release` is the irreversible event, and everything hangs
off it.** Choosing *patch*, *minor* or *major* from the Actions tab raises the
version, commits it, builds the four target artifacts, creates the GitHub
release, publishes the Homebrew formula to the tap, and runs
`cargo publish --workspace`. A crates.io publish is permanent: `cargo yank`
hides a version, it does not delete it, and the name stays taken. So §0 to §4
below run **before** the dispatch, against `dist build` output rather than
against a published artifact, and §5 verifies what landed after it.

**`git tag && git push --tags` no longer releases anything.** `dispatch-releases`
removed that trigger, for the reason `SPEC.md` §9 records: it is what lets the
bump start the release without a second permanent token.

The gate moved twice, and both moves were the same correction. This file first
said "before the first `publish`", which was true while the publish was a
command somebody typed; it became a CI job on 2026-08-08, so the last human
decision point moved to the tag. It moved again on 2026-08-09 when the tag
became a button. **Rehearse it rather than trusting this paragraph**: the bump's
`rehearse` option, or `release.yml`'s own dispatch with the tag left at
`dry-run`, runs the whole path and publishes nothing.

Every box carries evidence in the release notes: the command run and what it
printed. A checked box with no evidence is a claim, and this repo's method is
that a claim without a failing-capable check is a wish.

## 0. Prerequisites, once, before the first release ever

Three of these cannot be set by anything but a person holding a token, and a
release dispatched without them half fails: the binaries exist, the announcement
does not, and the crate name is still unclaimed. The third fails better than
that, stopping the release before anything is spent rather than half way
through, and it is still worth not discovering on the day.

- [x] `breferrari/homebrew-tap` exists and is public. *(Created 2026-08-08.)*
- [x] `gh secret set CARGO_REGISTRY_TOKEN` on `breferrari/vigia`, from a
      crates.io token scoped to `publish-new` and `publish-update` on `vigia`
      and `vigia-core`. *(Set 2026-08-09.)* `.github/workflows/publish-crates-io.yml`
      checks for it before packaging anything, so a missing one fails in seconds
      rather than several minutes in.
- [x] `gh secret set HOMEBREW_TAP_TOKEN` on `breferrari/vigia`. The job checks
      out the tap and pushes a commit, so contents read/write on
      `breferrari/homebrew-tap` is enough; dist's own guide asks for a classic
      token with `repo`, which is wider than the job needs. *(Set 2026-08-09.)*
- [ ] `gh secret set RELEASE_TOKEN` on `breferrari/vigia`, from a fine-grained
      token with **Contents: Read and write** on `breferrari/vigia` and nothing
      else. **Two properties are load bearing here and they are separate
      claims.** Contents is what lets the token write at all; its owner being an
      **admin** of the repository is what lets that write past `main`'s seven
      required status checks, which nothing else can do: a commit pushed with
      `GITHUB_TOKEN` triggers no workflow, so the checks it needs never arrive
      and the push is rejected forever. `bump.yml` proves both before the
      version moves, the first by creating a ref and deleting it, the second by
      reading the owner's role and the branch's `enforce_admins` setting.

## 1. The artifact, not the checkout

- [ ] `cargo package --list -p vigia` — no `.github/`, no `tests/`, and
      `README.md` present. SPEC.md §9 counts thirteen test files that read
      outside the package, and `exclude = ["tests/**"]` is what keeps them out of
      the tarball. Gated by
      `crates/vigia/tests/package.rs::the_packaged_artifact_carries_no_tests`,
      re-checked here because that gate skips when the registry index is
      unreachable.
- [ ] Unpack the built `.crate` into a clean directory; `cargo build --release`
      there succeeds with no path leaking back into the checkout.
- [ ] `dist plan` names all four targets (`x86_64-unknown-linux-musl`,
      `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`),
      three installers, and the tap rather than `homebrew-core`.
- [ ] `dist build --artifacts=lies` and read `target/distrib/vigia.rb`: the
      Linux URL names the **musl** archive. The formula's `target_triple` helper
      says `unknown-linux-gnu`, which is used only for binary aliases and is not
      what the install fragments resolve, so this is worth reading rather than
      assuming.

## 2. Install the way a user does, on every platform the release builds for

- [ ] `cargo install --path <unpacked crate>` (or the dist artifact) on Windows,
      macOS, Linux: binary lands on PATH, and `vigia --version` prints the
      release version. That flag exists as of #12; SPEC.md §11 B6 records why a
      version query is not the kind of flag it forbids.
- [ ] Binary size within the documented budget (SPEC.md §10 records 5.04 MiB
      with bundled grammars; a surprise here is a packaging change, not drift).
- [ ] musl artifact: `ldd` reports no shared libraries. The static claim is
      enforced in CI and re-checked here because this is the artifact, not the
      build.

## 3. Run it against a real repository, not a fixture

- [ ] Open on a real worktree with changes: first paint under the I7 feel test
      (instant), file list + diff drawn, header names the worktree.
- [ ] Edit a file while it watches: the change lands without input, follow
      works, `f` re-engages after a scroll.
- [ ] Quit with `q` AND with Ctrl-C: terminal restored both times, no raw-mode
      residue.
- [ ] Kill it from outside and look at the terminal it was in. Unix:
      `kill <pid>` from another pane. Windows: Ctrl+Break in the pane it is
      running in. Prompt, echo and cursor all back, and no mouse-report
      garbage when the pointer moves.
      ([#24](https://github.com/breferrari/vigia/issues/24) landed this and its
      gate signals a child process, so what is left here is the half a gate
      cannot reach: a real terminal, and on Windows a real key, which is the
      one delivery path #24 could not measure.)
- [ ] A non-repository path: one-line error before the alternate screen, exit
      non-zero.
- [ ] An option that does not exist: `vigia --colour=never`
      prints the one-line refusal and exits non-zero, rather than reporting that
      `--colour=never` is not a repository.
- [ ] A second argument: `vigia . --colour=never` says how many it got and
      exits non-zero, rather than watching `.` and dropping the flag. Both
      refusals go to stderr with nothing on stdout, so a script reading
      `vigia --version` is never handed an error message.

Three kills are deliberately **not** boxes here. `kill -9` and `taskkill /F` are
outside I8 on both platforms, because neither runs any code the process owns, and
the release notes say that rather than implying more. A *second* kill is inside
I8 as a by-choice exclusion (SPEC.md section 11.1: it takes the default
disposition and restores nothing), and it is covered by
`a_second_external_signal_kills_a_shell_that_ignored_the_first` rather than by a
box that a working build can never tick.

## 4. The claims the README makes are the claims the evidence holds

- [ ] "Flat resources over days" appears only if the 24-hour window has
      actually run ([#47](https://github.com/breferrari/vigia/issues/47));
      otherwise the README states the window that has.
- [ ] The mockup and the shell agree at the widths the README shows (the two
      deliberate departures SPEC.md §5.1 records are the only ones).
- [ ] Windows posture (supported vs best-effort) is stated, per SPEC.md §10's
      open half.
- [ ] The install section names only channels this release actually produces.
      **The README ships inside every artifact** (`dist plan` lists it under
      `[misc]` in each archive), so it describes the release it is packaged with
      rather than the state of the repository on the day it was edited.

## 5. After the dispatch

The publish is a CI job now, so these verify rather than perform.

- [ ] The `Release` workflow is green end to end, including
      `custom-publish-crates-io`. **Read that job specifically rather than the
      overall tick.** The GitHub release is created in `host`, before the
      registry job runs and with no `--draft`, so binaries being public proves
      nothing about crates.io. A green `announce` does not either: it is a
      checkout.
- [ ] `cargo install vigia` from crates.io, on one machine that has never built
      this repo. The true cold path.
- [ ] If the registry job failed while the release went public, that is the
      documented half-failure. **Which recovery depends on how far it got, and
      re-running the job is only right for one of the two cases**, because
      publishing an already-published version is an error rather than a silent
      no-op:
      - Nothing was accepted: re-run the job.
      - `vigia-core` was accepted and `vigia` was not: a plain re-run fails on
        `vigia-core` and never reaches `vigia`. Publish the second by hand,
        `cargo publish -p vigia --locked`, from the tagged commit.

      Either way `vigia-core` 0.1.0 is spent permanently once it is accepted, so
      the fix is never to bump one crate and not the other.
- [ ] `brew install breferrari/tap/vigia`, and the formula in the tap names the
      tag that was just pushed.
- [ ] The GitHub release carries the artifacts `cargo-dist` built, not a
      re-build, and the tag matches the SHA that was smoke-tested above.
