# take-next rules for vigia

draft-runs: skip

`ci.yml` skips every job until a pull request is ready, so the line above makes `checks.sh` count only runs made after `gh pr ready`.

## 5. Scope the checks

A code diff also runs the doc build in `CONTRIBUTING.md`'s command list, before `gh pr ready` and before any later push. CI fails its `fmt and docs` leg on a doc warning, such as a public doc that links a private item, and no other local check catches one.
