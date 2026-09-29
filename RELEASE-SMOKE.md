# Release smoke — run against the built artifact, before the release is dispatched

> [!IMPORTANT]
> **This is not how a release is performed.** The release is one command, and it lives in `CLAUDE.md`: dispatch the `bump and release` workflow. This file is the **human pre-flight** that goes with it. Most of its boxes need a person at a terminal on three platforms, who kills the process from another pane and watches what the terminal does next. An agent cannot tick anything here.
>
> Read it before a release that changes packaging, installation or the terminal takeover. It is not a gate to clear before every dispatch. Treating it as the procedure has cost a session: it is the only release-shaped document in the repository root, so a reader finds it first.

CI green is necessary and not sufficient. A sibling project shipped two consecutive patches with a green matrix that broke the flagship install on day one, and its fix was the ancestor of this checklist.

**Dispatching `bump and release` is the irreversible event, and everything hangs off it.** You choose *patch*, *minor* or *major* from the Actions tab. The workflow raises the version, commits it, builds the four target artifacts, creates the GitHub release, publishes the Homebrew formula to the tap, and runs `cargo publish --workspace`. A crates.io publish is permanent: `cargo yank` hides a version, it does not delete it, and the name stays taken. So §0 to §4 below run **before** the dispatch, against `dist build` output and not against a published artifact, and §5 makes sure that what landed is correct after it.

**`git tag && git push --tags` no longer releases anything.** `dispatch-releases` removed that trigger, for the reason `SPEC.md` §9 records: it is what lets the bump *start* the release without a second permanent token. Starting the release and committing the bump are different questions, and that section answers only the first. The bump's own commit to a protected `main` does need a token of its own, which is the third secret in §0.

The gate moved twice, and both moves were the same correction. This file first said "before the first `publish`". That was true while a person typed the publish. The publish became a CI job on 2026-08-08, so the last human decision point moved to the tag. It moved again on 2026-08-09 when the tag became a button. **Rehearse the release and do not trust this paragraph**: the bump's `rehearse` option, or the dispatch of `release.yml` itself with the tag left at `dry-run`, runs the whole path and publishes nothing.

Every box carries evidence in the release notes: the command you ran and what it printed. A checked box with no evidence is a claim. This repo's method is that a claim without a check that can fail is a wish.

## 0. Prerequisites, once, before the first release ever

Five boxes. The first is that the tap repository exists at all. Two are secrets, and only a person who holds a token can set them. A release dispatched without either secret fails, with the binaries built, the announcement missing and the crate name still unclaimed. The last two are not secrets. One is a repository setting that the release depends on and deliberately does not check. One is a claim that nobody has yet been able to prove.

**Two secrets, three jobs, because one token does two of them.** `HOMEBREW_TAP_TOKEN` carries Contents read and write on *both* `breferrari/homebrew-tap` and `breferrari/vigia`. So it is the credential that pushes the formula to the tap **and** the credential that moves the version on `main`. `bump.yml` reads it into a variable named for the second role, because that is what the step reasons about. The name on the secret is older than the second use. The team leaves it alone instead of changing it. To rename a secret you must regenerate the token. You can separate the two uses whenever you want: mint a token scoped to this repository alone and point the bump at it.

- [x] `breferrari/homebrew-tap` exists and is public. *(Created 2026-08-08.)*
- [x] `gh secret set CARGO_REGISTRY_TOKEN` on `breferrari/vigia`, from a
crates.io token scoped to `publish-new` and `publish-update` on `vigia` and `vigia-core`. *(Set 2026-08-09.)* `.github/workflows/publish-crates-io.yml` checks for it before it packages anything, so a missing token fails in seconds and not several minutes in.
- [x] `gh secret set HOMEBREW_TAP_TOKEN` on `breferrari/vigia`, from a
fine-grained token with **Contents: Read and write** on **both** `breferrari/homebrew-tap` and `breferrari/vigia`, **owned by an account that is an admin of `breferrari/vigia`**. *(Set 2026-08-09; the vigia grant recorded here 2026-08-12.)* The guide for dist asks for a classic token with `repo`, which is wider than either job needs.

The tap half needs only contents on the tap. **The bump half needs two things that are separate claims.** Contents on this repository lets the token write at all. The token owner being an admin lets that write get past the seven required status checks of `main`. Nothing else can do this. A commit pushed with `GITHUB_TOKEN` triggers no workflow, so the checks it needs never arrive and the push is rejected forever. `bump.yml` makes sure that both claims hold before the version moves. It checks the first by creating a ref and deleting it, and the second by reading the role of the owner. It probes the tap the same way. The two probes still tell the two apart although they share a credential, because the list of repositories on the token can drop either repository on its own.
- [ ] `main` keeps **"do not allow bypassing the above settings" switched off**,
which is what makes a push by an admin legal at all. `bump.yml` does not check this, and the reason is that reading branch protection needs admin rights on the API. The token of the workflow cannot get those rights, and it is better to leave the release token unwidened than to give it them. If the setting is ever switched on, the release fails at the push with `main` unmoved and nothing published.
- [ ] **Unproven until the first real push:** that a fine-grained token
*inherits* the bypass of its owner. The documentation says it does, and the whole button rests on it as a premise, so it is written here as a claim and not left implied. The team measured the two halves either side of it. An admin identity does bypass these exact seven checks (probed on a throwaway branch protected identically to `main`, 2026-08-11), and every run probes the write grant of the token. Tick this after a real bump push to `main` lands, and name the run.

## 1. The artifact, not the checkout

- [ ] `cargo package --list -p vigia`: no `.github/`, no `tests/`, and
`README.md` present. SPEC.md §9 counts thirty test files that read outside the package, and `exclude = ["tests/**"]` keeps them out of the tarball. `crates/vigia/tests/package.rs::the_packaged_artifact_carries_no_tests` gates this. Check it here again, because that gate skips when the registry index is unreachable.
- [ ] Unpack the built `.crate` into a clean directory. `cargo build --release`
there succeeds with no path that leaks back into the checkout.
- [ ] `dist plan` names all four targets (`x86_64-unknown-linux-musl`,
`x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`), three installers, and the tap and not `homebrew-core`.
- [ ] `dist build --artifacts=lies` and read `target/distrib/vigia.rb`: the
Linux URL names the **musl** archive. The `target_triple` helper of the formula says `unknown-linux-gnu`. It uses that value only for binary aliases, and the install fragments do not resolve to it. Read the file and do not assume this.

## 2. Install the way a user does, on every platform the release builds for

- [ ] `cargo install --path <unpacked crate>` (or the dist artifact) on Windows,
macOS, Linux: the binary lands on PATH, and `vigia --version` prints the release version. That flag exists as of #12. SPEC.md §11 B6 records why a version query is not the kind of flag it forbids.
- [ ] The binary size is within the documented budget (SPEC.md §6 records 6,102,528 bytes
with 217 syntaxes). A surprise here is a packaging change, not drift.
- [ ] musl artifact: `ldd` reports no shared libraries. CI enforces the static claim,
and you check it here again because this is the artifact, not the build.

## 3. Run it against a real repository, not a fixture

- [ ] Open it on a real worktree with changes: first paint under the I7 feel test
(instant), file list and diff drawn, header names the worktree.
- [ ] Edit a file while it watches: the change lands without input, follow
works, `f` re-engages after a scroll.
- [ ] Edit a file whose diff is **taller than the pane**, low down in it: the
change itself is on screen, not the filename above it. [#257](https://github.com/breferrari/vigia/issues/257) is visible only in that shape, and a fixture cannot judge it. The question is whether a reader who glances over sees what the agent just did.
- [ ] Press `?`, then **resize the pane through the sheet's own ladder** and
watch it. Do not open it once on one size. Start full-screen, where a pane 70 columns or wider with 31 rows of body draws the roomy rung, the one with sections and air in it. Shorten the pane until the sections and their air go and the table closes up. Narrow it until the table takes the tight spellings. Shorten it until the mouse group moves *beside* the keyboard group and not below it (on a hundred-column pane, 19 to 23 rows high is the two-column rung and 24 to 33 is one column). Then shorten it further until rows start dropping. Nothing must jump, tear, or straddle the header or the footer. `?` must still take the sheet away at every size. A gate walks 105 widths by 33 heights and can read only cells. Whether each snap *reads* as one element that changes shape or as three different boxes is a judgement that only an eye can make. The roomy-to-plain snap is the largest of them: 68 by 31 to 56 by 21. ([#220](https://github.com/breferrari/vigia/issues/220) added the two-column rung and [#285](https://github.com/breferrari/vigia/issues/285) the roomy one. [#286](https://github.com/breferrari/vigia/issues/286) will move the floor again, so this box outlives them.)
- [ ] Quit with `q` AND with Ctrl-C: the terminal is restored both times, with no raw-mode
residue.
- [ ] Kill it from outside and look at the terminal it was in. Unix:
`kill <pid>` from another pane. Windows: Ctrl+Break in the pane where it runs. The prompt, echo and cursor all come back. No mouse-report garbage appears when the pointer moves. ([#24](https://github.com/breferrari/vigia/issues/24) landed this, and its gate signals a child process. What is left here is the half that a gate cannot reach: a real terminal, and on Windows a real key, which is the one delivery path that #24 was unable to measure.)
- [ ] A non-repository path: a one-line error before the alternate screen, and a
non-zero exit.
- [ ] An option that does not exist: `vigia --colour=never`
prints the one-line refusal and exits non-zero. It does not report that `--colour=never` is not a repository.
- [ ] A second argument: `vigia . --colour=never` says how many arguments it got and
exits non-zero. It does not watch `.` and drop the flag. Both refusals go to stderr with nothing on stdout, so a script that reads `vigia --version` never gets an error message.

- [ ] **Drag the diff, let go, and paste somewhere else.** Do this on each platform the
release builds for, because the tool that carries the copy is different on every one: `pbcopy` on macOS, `wl-copy` or `xclip` or `xsel` on Linux, `clip` on Windows. **Nothing in the suite reaches this.** The tests drive every route through a carrier that the tests implement. So the tests prove the order in which the routes are tried and the shape of each command. They never prove that one of the routes set a clipboard.
- [ ] **The same inside `tmux`, and again over `ssh`.** The routing exists for these
two cases, and a local run cannot reach either. Inside tmux at the machine where you sit, the tool of that machine must carry the copy. tmux must never be asked. Over `ssh` the routing skips that tool on purpose, because when it runs there it sets a clipboard on the far end and succeeds at it. So the copy goes to tmux where there is a pane, and to the escape otherwise. That last path depends on the `set-clipboard` setting of the reader and on the outer terminal that speaks OSC 52, which is what the block in the README tells the reader to check. **A paste that returns what was copied before the drag is this failing**, and this is exactly how [#477](https://github.com/breferrari/vigia/issues/477) was reported.

Three kills are deliberately **not** boxes here. `kill -9` and `taskkill /F` are outside I8 on both platforms, because neither runs any code that the process owns. The release notes say that and imply nothing more. A *second* kill is inside I8 as an exclusion by choice (SPEC.md section 11.1: it takes the default disposition and restores nothing). `a_second_external_signal_kills_a_shell_that_ignored_the_first` covers it, and no box covers it, because a working build can never tick such a box.

## 4. The claims the README makes are the claims the evidence holds

- [ ] "Flat resources over days" appears only if the 24-hour window has
actually run ([#47](https://github.com/breferrari/vigia/issues/47)). Otherwise the README states the window that has run.
- [ ] The mockup and the shell agree at the widths the README shows (the two
deliberate departures that SPEC.md §5.1 records are the only differences).
- [ ] The Windows posture (supported or best-effort) is stated, per the open half of
SPEC.md §10.
- [ ] The install section names only channels that this release actually produces.
**The README ships inside every artifact** (`dist plan` lists it under `[misc]` in each archive), so it describes the release that it is packaged with and not the state of the repository on the day someone edited it.

## 5. After the dispatch

The publish is a CI job now, so these steps make sure that the publish is correct. They do not perform it.

- [ ] The `Release` workflow is green end to end, including
`custom-publish-crates-io`. **Read that job specifically and not the overall tick.** The GitHub release is created in `host`, before the registry job runs and with no `--draft`, so public binaries prove nothing about crates.io. A green `announce` does not prove it either: that job is a checkout.
- [ ] `cargo install vigia` from crates.io, on one machine that has never built
this repo. This is the true cold path.
- [ ] If the registry job failed while the release went public, that is the
documented half-failure. **The recovery depends on how far the job got, and a re-run of the job is right for only one of the two cases**, because publishing a version that is already published is an error and not a silent no-op:
      - Nothing was accepted: re-run the job.
      - `vigia-core` was accepted and `vigia` was not: a plain re-run fails on
`vigia-core` and never reaches `vigia`. Publish the second by hand, `cargo publish -p vigia --locked`, from the tagged commit.

Either way `vigia-core` 0.1.0 is spent permanently once the registry accepts it, so the fix is never to bump one crate and not the other.
- [ ] `brew install breferrari/tap/vigia`, and the formula in the tap names the
tag that was just pushed.
- [ ] The GitHub release carries the artifacts that `cargo-dist` built, not a
re-build, and the tag matches the SHA that was smoke-tested above.
