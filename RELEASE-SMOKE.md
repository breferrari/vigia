# Release smoke — run against the built artifact, before the tag

CI green is necessary and not sufficient. A sibling project shipped two
consecutive patches with a green matrix that broke the flagship install on day
one, and its fix was this checklist's ancestor.

**The `git tag` is the irreversible event, and everything hangs off it.** Pushing
`v0.1.0` builds the four target artifacts, creates the GitHub release, publishes
the Homebrew formula to the tap, and runs `cargo publish --workspace`. A
crates.io publish is permanent: `cargo yank` hides a version, it does not delete
it, and the name stays taken. So §0 to §4 below run **before** the tag, against
`dist build` output rather than against a published artifact, and §5 verifies
what landed after it.

That ordering changed on 2026-08-08 with
[#12](https://github.com/breferrari/vigia/issues/12). This file used to say
"before the first `publish`", which was true while the publish was a command
somebody typed. It is a CI job now, so the last human decision point moved
earlier, to the tag.

Every box carries evidence in the release notes: the command run and what it
printed. A checked box with no evidence is a claim, and this repo's method is
that a claim without a failing-capable check is a wish.

## 0. Prerequisites, once, before the first tag ever

Two of these cannot be set by anything but a person holding a token, and a tag
pushed without them produces a release that half fails: the binaries exist, the
announcement does not, and the crate name is still unclaimed.

- [ ] `breferrari/homebrew-tap` exists and is public. *(Created 2026-08-08.)*
- [ ] `gh secret set CARGO_REGISTRY_TOKEN` on `breferrari/vigia`, from a
      crates.io token with publish scope. `.github/workflows/publish-crates-io.yml`
      checks for it before packaging anything, so a missing one fails in seconds
      rather than several minutes in.
- [ ] `gh secret set HOMEBREW_TAP_TOKEN` on `breferrari/vigia`, from a GitHub
      personal access token with `repo` scope. This is what lets the release
      write to the tap.

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

## 2. Install the way a user does, on all three tier-1 targets

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
- [ ] An argument that is not an option and not a path: `vigia --colour=never`
      prints the one-line refusal and exits non-zero, rather than reporting that
      `--colour=never` is not a repository.

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

## 5. After the tag

The publish is a CI job now, so these verify rather than perform.

- [ ] The `Release` workflow is green end to end, including
      `custom-publish-crates-io`. `announce` waits on it, so a green
      announcement is itself evidence the registry accepted both crates.
- [ ] `cargo install vigia` from crates.io, on one machine that has never built
      this repo. The true cold path, and the only box here that a green workflow
      does not already imply.
- [ ] `brew install breferrari/tap/vigia`, and the formula in the tap names the
      tag that was just pushed.
- [ ] The GitHub release carries the artifacts `cargo-dist` built, not a
      re-build, and the tag matches the SHA that was smoke-tested above.
