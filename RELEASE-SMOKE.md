# Release smoke — run against the built artifact at the release SHA

CI green is necessary and not sufficient. A sibling project shipped two
consecutive patches with a green matrix that broke the flagship install on day
one, and its fix was this checklist's ancestor. A crates.io publish is
permanent — `cargo yank` hides a version, it does not delete it, and the name
stays taken — so everything below runs BEFORE the first `publish`, against the
artifact a user would receive, never against the checkout that built it.

Every box carries evidence in the release notes: the command run and what it
printed. A checked box with no evidence is a claim, and this repo's method is
that a claim without a failing-capable check is a wish.

## 1. The artifact, not the checkout

- [ ] `cargo package --list` — no `.github/`, no test support paths (SPEC.md §9
      records the two escapes that read outside the package; confirm the
      exclusion actually excludes them).
- [ ] Unpack the built `.crate` into a clean directory; `cargo build --release`
      there succeeds with no path leaking back into the checkout.
- [ ] `cargo-dist` dry run: every tier-1 artifact builds; the Homebrew formula
      it generates names the tap, not `homebrew-core`.

## 2. Install the way a user does, on all three tier-1 targets

- [ ] `cargo install --path <unpacked crate>` (or the dist artifact) on Windows,
      macOS, Linux — binary lands on PATH, `vigia --version` prints the release
      version.
- [ ] Binary size within the documented budget (SPEC.md §10 records 5.04 MiB
      with bundled grammars; a surprise here is a packaging change, not drift).
- [ ] musl artifact: `ldd` reports no shared libraries (the static claim is
      enforced in CI and re-checked here because this is the artifact, not the
      build).

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

## 5. After the publish

- [ ] `cargo install vigia` from crates.io, on one machine that has never built
      this repo — the true cold path.
- [ ] `brew install breferrari/tap/vigia` once the tap exists.
- [ ] Tag matches the published SHA; the GitHub release carries the artifacts
      `cargo-dist` built, not a re-build.
