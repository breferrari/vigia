# vigia

A live diff **monitor** for the terminal. Not a review tool.

`vigia` is Portuguese: a watchman, and — nautically — a porthole. The window you
look through. Also the verb: `vigia .` reads as "watch this."

## What this is, in one paragraph

You run an AI coding agent in one pane and `vigia` in the pane beside it. It
shows the working-tree diff as it changes, continuously, without being touched.
It is ambient. It is closer to `btop` than to a git client: you read state from
shape and colour, glance away, and glance back.

## The product class is the whole thesis

`vigia` is **monitor-class**: already open, rarely touched, correct with zero
input, cheap for days, glanceable. It is not a **reviewer** — something you
launch per changeset to step through, annotate and decide on.

That distinction is not stylistic. It generates every budget in `SPEC.md`, and
those budgets are the product.

**If a change would make `vigia` a better reviewer at the cost of an invariant,
the change is wrong.** Reject it and say why.

## Stack — settled, do not relitigate

| Layer | Choice | Why |
|---|---|---|
| TUI | `ratatui` + `crossterm` | `bottom`, a btop-class system monitor, runs exactly this pair. `crossterm` is the only backend with Windows plus cross-platform mouse; `termion` is Unix-only |
| Git | `gix` | In-process diff. No `git diff` subprocess per tick |
| Highlighting | `syntect` | Pure Rust, so no C toolchain in CI. What `delta` and `bat` use |
| Release | `cargo-dist` | Cross-platform binaries + Homebrew formula + GH workflow |

Everything above is pure Rust on purpose: `--target x86_64-unknown-linux-musl`
gives a static binary with no cross-toolchain, and macOS/Windows are tier-1.
**Choosing tree-sitter over `syntect` reintroduces a C toolchain** — that is a
spec change, not an implementation detail.

`gix` is the least-precedented dependency here (`delta` uses `git2`/libgit2
instead, likely for age reasons). **Prove `gix` first.**

## Method: spec-driven, drift-enforced

`SPEC.md` is the source of truth. Code is written against it.

1. If the code and `SPEC.md` disagree, **stop.** One of them is wrong; decide
   which, change that one deliberately, in its own commit.
2. Every invariant in `SPEC.md` has a test that fails when it is violated.
   An invariant without a failing test is a wish.
3. Performance budgets are tests, not aspirations. The thesis is a measurable
   claim, so a regression past budget **fails the build**.

Do not add a dependency, a flag, or a subcommand that `SPEC.md` does not name.
Propose it, get it into the spec, then build it.

## Where design decisions live

Design rationale for this project is recorded outside this repo, reachable
through the **`vigil` MCP server**. This repo declares its identity as `vigia` in
`.om-project`, which is what scopes reads and writes to this project.

### Reading

| Tool | Use it for |
|---|---|
| **`search`** | The full written record: why the product class is what it is, why each dependency was chosen, which alternatives were rejected and on what evidence, what each budget was set against. **Start here.** |
| **`expand`** | Once you have a specific note, see what it links to and what links back. Cheaper and more exact than searching again for the neighbourhood. |
| **`recall`** | Short durable lessons scoped to this project. Pass `explain: true` when something you expected is missing: it distinguishes "scoped away" from "never existed". |
| **`reason`** | A judgement that needs several notes weighed against each other, e.g. "is what I am about to do consistent with what was decided". It spawns a second session, so it is slower and costs more. Reach for it only when `search` returned the notes but not the answer. |
| **`health`** | When something that should be there cannot be found. Every failure in this layer looks identical from the outside (no results), and this is what tells them apart. |
| Resources | Notes are also exposed as `vault://note/<path>` and can be read directly when the title already answers the question. |

**`recall` is empty until sessions put things in it.** Early on it returns
nothing, and that is *not* evidence the record is missing. Do not conclude "there
is no record" from an empty `recall`. Use `search`.

Consult the record before changing:

- the CLI surface (flags, subcommands, exit codes)
- the rendering contract (what a "diff view" includes and excludes)
- the file-watching strategy and its platform assumptions
- the performance budgets and what they were set against

If that record and this repo disagree, the record holds the *why*. Reconcile
before changing behaviour.

### Writing

Two tools, and picking the wrong one is the common mistake. **The test is whether
it would help someone working on a different project.**

**`remember`** stores a durable **lesson**: a constraint you discovered, a gotcha
that cost time, a rule that generalises. Not status, not a task summary, not
anything you would resent being told again in six weeks.

- `confidence` is `verified` | `inferred` | `unverified`. Be honest, it is what a
  reader trusts. Supply `verification` whenever you claim `verified`: how you
  know, i.e. the test you ran or the source you read.
- `scope` is `project` | `platform` | `general`. For something specific to this
  tool use `scope: "project"` with `projects: ["vigia"]`. Use `general` only when
  it genuinely applies everywhere.
- `links` connects it to existing notes by title. `supersedes` corrects an
  earlier memory: the old one is kept and back-linked rather than deleted.
- `dry_run: true` previews first.

**`record_work`** files what happened **here** into the vault. Use it at the end
of a real piece of work. Write it for a session that will not have your context
and cannot re-read your diff, so fill every field you can: `summary`, `changes`
(one line per file, what and why), `decisions` (especially where you rejected an
alternative), `learned` (surprises and near-misses), `open` (unresolved threads
nobody should assume are handled), `verification` (tests run and their result,
failures stated honestly). `kind: "decision"` files it as a decision record;
`informed_by` credits the notes you actually read.

Rule of thumb: **a `gix` limitation that would bite any Rust project is a
`remember`. "Landed the watch engine and here is what it cost" is a
`record_work`.** Do both when both are true.

Before finishing work that changed or clarified a decision here, write it down.
A finding that stays in this session is a finding the next session pays for
again.

## House rules

- **No agent-session artifacts in anything that lands in the repo.** No
  `claude.ai/code/session_*` URL, no `Claude-Session:` trailer, no local
  absolute path — not in commit messages, PR or issue bodies, or files.
  `Co-Authored-By:` is fine and wanted.
- **No em-dashes** in anything published under Brenno's name: README prose,
  release notes, issue and PR bodies, commit messages. Use a period, a comma,
  a colon, or parentheses.
- Probe capability by behaviour, never by asking. A single green run is not
  evidence when the defect is non-deterministic.
- Verify the whole artifact, not just the property you were fixing.
