# Contributing to vigia

Issues and pull requests are welcome. This file is the whole of what you need to know before opening one, so a first contribution fails on nothing by surprise.

## The short version

- **A plain bug report is welcome.** Two lines is enough: what you expected, what happened. Nothing here asks you to read the spec first.
- **Before writing code, read `SPEC.md`.** It is long. That is the deal, and the reason is below.
- **Discussions are open** for anything that is not yet a bug or a proposal.

## Reporting something

Open an issue. What helps most:

- your terminal and its version, your OS, and `vigia --version`
- what you expected to see and what the pane actually drew
- a screenshot if it is about what the pane looks like

**Reports from use are the scarcest thing this project has.** Most of the open issues were found by an audit or a mutation pass rather than by somebody watching the tool work. A report that begins *"I was watching an agent write and…"* is worth more than a well-formed proposal.

## Before you write code

**`SPEC.md` is read before code, by everyone.** It is the contract: what must hold now, and why. Most of what looks like a free choice in this codebase is already ruled on, with the measurement that decided it recorded beside the rule.

That is a real ask, and it is the honest one: a change that contradicts a ruling will be sent back, and the spec is the only place the rulings live. Start with:

- **§3 Invariants** — the eleven claims everything else is built to keep
- **§11.1** — what the shell does today, rule by rule
- **§11.2** — questions that are open, and what was ruled on the ones that are not

`ROADMAP.md` says what is next. `RULINGS.md` is the evidence trail behind the rulings — you do not need it to write code, only to argue with a ruling.

## House rules

These are the ones a first PR is most likely to trip on.

- **Comments explain what the code cannot.** No issue numbers, no dates, no record of what an earlier draft of the comment said. A reference to `§11.1`, a `B<n>` ruling or an `I<n>` invariant is welcome and is the exception: it says *this code implements that rule*. `crates/vigia/tests/register.rs` gates all three.
- **A docblock longer than the item it documents means one of the two is wrong.**
- **An invariant without a failing test is a wish.** Nothing lands until a test fails when it is violated. If you cannot make it fail, say so in the PR and we will work out the gate together.
- **Numbers or it did not happen.** A type signature is not evidence and a single green run is not evidence.
- **Pure Rust.** Any dependency pulling `cc`, `cmake` or `bindgen` breaks static Linux builds and Windows, and CI fails the build if one appears.
- **Do not hard-wrap prose.** Commit message bodies wrap at 72; markdown and PR bodies do not wrap at all, because GitHub renders a single newline as a line break.
- **Titles say what is broken or what to build.** One clause, no "because" — the explanation goes in the body.

## Running things

```sh
cargo test --workspace          # everything, including the budget gates
cargo clippy --workspace --all-targets
cargo fmt --all
```

CI runs on Linux, macOS and Windows, with a musl leg for the artifact Linux actually ships. The budget gates run in debug on every commit; the absolute wall-clock tier is release-only.

## What this project will not do

`ROADMAP.md`'s "Non-goals, permanent" is the list, and it is permanent rather than "later": staging and committing, branch browsing, comment threads, AI features, remote operations, and a GUI. Each is reviewer-class work or costs an invariant. A proposal for one of those will be declined, and the decline is not a judgement on the idea.

## Licence

By contributing you agree that your contribution is licensed under the same terms as the project.
