# take-next rules for vigia

draft-runs: skip

`ci.yml` skips every job while a pull request is a draft, so a run created before the ready call tested nothing. The line above makes `checks.sh` count only runs created after it.

## 5. Scope the checks

A code diff also runs the doc build CI runs, before `gh pr ready` and before any push after it: `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`. CI fails the `fmt and docs` leg on a doc warning, such as a public doc that links a private item, and nothing else the scope runs finds one.
