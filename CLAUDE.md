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
through the `vigil` MCP server. Two tools, holding different things:

- **`search`** reaches the full written record: why the product class is what it
  is, why each dependency was chosen, which alternatives were rejected and on
  what evidence, what each budget was set against. **Start here.** Search for
  the decision you are about to touch, then read the note it returns.
- **`recall`** returns short durable constraints for this project. It is
  **empty until sessions put things in it**, so early on it will return nothing
  and that is not a signal the record is missing. It fills as work happens, via
  `remember` below, and once populated it is the cheaper first call.

Do not conclude "there is no record" from an empty `recall`. Use `search`.

Consult it before changing:

- the CLI surface (flags, subcommands, exit codes)
- the rendering contract (what a "diff view" includes and excludes)
- the file-watching strategy and its platform assumptions
- the performance budgets and what they were set against

If that record and this repo disagree, the record holds the *why*. Reconcile
before changing behaviour.

## Recording what you learn

Before finishing work that changed or clarified a decision here, record it with
`remember`. A finding that stays in this session is a finding the next session
pays for again.

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
