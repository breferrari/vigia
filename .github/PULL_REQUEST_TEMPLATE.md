<!--
Thanks for the pull request. CONTRIBUTING.md has the house rules. The short version:

Title: one clause, imperative. Internal work starts with its kind: ci:, docs:, spec:, roadmap:, skill:, test: or chore:. A change that ships in the binary starts with fix: for a defect or feat: for a new feature. Its title can become the changelog line.
-->

## Summary

<!-- What this changes and why. Link the issue: closes #123. -->

## Test plan

<!-- How you verified it: the tests you ran and their result, with numbers. A budget gate reports its number against the budget. State failures plainly. -->

## Release note

<!--
One line a user of vigia can read, describing what they see change, written as `Release-note: <sentence>`. Write `Release-note: none` for internal work such as CI, tests, docs or refactors. The line is read from a commit message body first, then from here.

The label is a maintainer's job if you cannot set it: `release` when the change ships in the binary (any file under `crates/*/src/` or `crates/*/assets/`, a dependency, installation), `internal` otherwise.
-->
