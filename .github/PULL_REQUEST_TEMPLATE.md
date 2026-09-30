<!-- Title: a short imperative, one clause. An internal PR starts with its kind: ci:, docs:, spec:, roadmap:, skill:, test: or chore:. A release PR has no prefix, because its title can become the changelog line. -->

## What is true now

One paragraph. What a user or a maintainer can do or see now that they could not before. Link the issue.

## Why

The problem this solves, and the alternative you did not take, if there was one.

## Release

- [ ] Label: `release` when a user of the pane can see the change, `internal` for everything else. A maintainer applies it if you cannot.
- [ ] A commit body carries `Release-note: <one sentence a user can read>`, or `Release-note: none` for internal work. The PR description does not count: it is lost on squash.

## Verification

The tests you ran and their result, with numbers. A budget gate reports its number against the budget. State failures plainly.
