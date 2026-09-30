# Contributing to vigia

Issues and pull requests are welcome. This file has everything you need to know before you open one, so a first contribution does not fail on any surprise.

## The short version

- **A plain bug report is welcome.** Two lines is enough: what you expected, what happened. Nothing here asks you to read the spec first.
- **Before writing code, read `SPEC.md`.** It is long. That is the deal, and the reason is below.
- **Discussions are open** for anything that is not yet a bug or a proposal.

## Reporting something

Open an issue. What helps most:

- your terminal and its version, your OS, and `vigia --version`
- what you expected to see and what the pane actually drew
- a screenshot if it is about what the pane looks like

**Reports from use are the scarcest thing this project has.** An audit or a mutation pass found most of the open issues, and not somebody watching the tool work. A report that begins *"I was watching an agent write and…"* is worth more than a well-formed proposal.

## Before you write code

**`SPEC.md` is read before code, by everyone.** It is the contract: what must hold now, and why. Most of what looks like a free choice in this codebase already has a ruling, with the measurement that decided it recorded beside the rule.

That is a real ask. A change that contradicts a ruling is sent back, and the spec is the only place the rulings live. Start with:

- **§3 Invariants**: the eleven claims that everything else is built to keep
- **§11.1**: what the shell does today, rule by rule
- **§11.2**: questions that are open, and what was ruled on the ones that are not

`ROADMAP.md` says what is next. `RULINGS.md` is the evidence behind the rulings. You do not need it to write code or to open your first PR. You need it only to argue with a ruling.

## House rules

A first PR is most likely to break these rules.

- **Comments explain what the code cannot.** No issue numbers, no dates, no record of what an earlier draft of the comment said. A reference to `§11.1`, a `B<n>` ruling or an `I<n>` invariant is welcome and is the exception: it says *this code implements that rule*. `crates/vigia/tests/register.rs` gates all three.
- **A docblock longer than the item it documents means one of the two is wrong.**
- **An invariant without a failing test is a wish.** Nothing lands until a test fails when it is violated. If you cannot make it fail, say so in the PR and we will work out the gate together.
- **Numbers or it did not happen.** A type signature is not evidence and a single green run is not evidence.
- **Pure Rust.** Any dependency that pulls `cc`, `cmake` or `bindgen` breaks static Linux builds and Windows, and CI fails the build if one appears.
- **Do not hard-wrap prose.** Markdown files, PR bodies and commit message bodies do not wrap at all: one paragraph is one line, because GitHub renders a single newline as a line break.
- **Titles say what is broken or what to build.** One clause, no "because". The explanation goes in the body. A pull request has three sections, summary, test plan and release note, and GitHub fills the template in for you.
- **A title can become a release note.** See the next section for how the release notes are built and what you control.

## Release notes

Every pull request carries one of two labels before it merges: `release` or `internal`. A maintainer applies it, so you do not need write access to contribute. If you can set labels, set it yourself.

- **`release`** means the change ships in the binary: any file under `crates/*/src/` or `crates/*/assets/`, a dependency, or how `vigia` is installed. A change a user cannot see, such as a frame that costs less, is still a release. The pull request is listed in `CHANGELOG.md` and in the GitHub Release.
- **`internal`** means everything else: CI, tests, documentation, the roadmap, the spec, scripts. The pull request is not listed anywhere.

What you control is the line a user reads. Put `Release-note: <one sentence>` in the Release note section of the pull request description, or in a commit message body on your branch, and that sentence becomes the changelog line instead of your title. Put `Release-note: none` when the change is internal. Write the sentence for someone who has never opened this repository: name what changed for them, not the rule behind it. If you write nothing, the title of a `release` pull request becomes the line, so make the title readable on its own.

The release reads the commit message first and the pull request description second, so a line in a commit wins. The CI check on wording, described below, reads commits only.

Start the title of an `internal` pull request with a prefix that says what kind of work it is: `ci:`, `docs:`, `spec:`, `roadmap:`, `skill:`, `test:`, or `chore:` for anything else. A `release` pull request starts with `fix:` for a defect or `feat:` for a new feature. Its title can become the changelog line, prefix included. The prefix is for people reading the log and the changelog. The label is what the release reads.

A release stops when a merged pull request carries no label, both labels, or a `Release-note:` line that disagrees with its label, such as `none` on a `release` pull request. The stop names the pull request so a maintainer can fix the label. It is not something you need to watch. If every change since the last release is internal, no version is released.

## Public files and contract files

Two kinds of document live here, and they are written differently.

- **Public files** are `README.md`, `CHANGELOG.md`, this file, the issue template and the pull request template. Release notes are copied from `CHANGELOG.md`. Write them in ordinary English: the fact first, short sentences, common words. Name what a user sees change, not the rule behind it.
- **Contract files** are `SPEC.md`, `RULINGS.md`, `REVOCATIONS.md`, `ROADMAP.md` and `CLAUDE.md`. They keep their own style: present tense, and each ruling names who made it and when.

Phrases from the contract style are not allowed in public files. `.github/public-dialect.txt` lists them, and a test checks it. CI also checks your PR title and commit subjects against that list, and fails unless a commit body carries a `Release-note:` line. CI does not rerun when you only edit the PR title, so push a commit after renaming.

## Running things

```sh
cargo nextest run --workspace   # the suite; cargo install cargo-nextest --locked
cargo test --workspace --doc    # the doctests, which nextest does not run
cargo clippy --workspace --all-targets
cargo fmt --all
```

CI runs on Linux, macOS and Windows, with a musl leg for the artifact that Linux actually ships. The budget gates run in debug on every commit. The absolute wall-clock tier runs in release only. A pull request that changes only documents, skills, templates or images runs the suites that read documents on Linux and skips the rest, so it finishes in a couple of minutes. The `full-ci` label forces the whole run on any pull request.

## What this project will not do

`ROADMAP.md`'s "Non-goals, permanent" is the list. It is permanent, not "later": staging and committing, branch browsing, comment threads, AI features, remote operations, and a GUI. Each is reviewer-class work or costs an invariant. The team will decline a proposal for one of those, and the decline is not a judgement on the idea.

## Licence

By contributing you agree that the project's licence terms also apply to your contribution.
