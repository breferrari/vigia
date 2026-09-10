# The tech pack

`techpack.yaml` at the root of this repository packages what the README asks you to set up by hand, so that [`mcs`](https://github.com/mcs-cli/mcs) installs and maintains it instead: the server from **Give the agent the server**, and the three hooks from **And reach the session already running**.

**macOS and Linux.** `mcs` installs the binary through Homebrew and the hooks are shell scripts, so this route does not reach Windows even though `vigia` itself is a tier-1 target there. A Windows reader wants the README's own instructions, which are three lines of `claude mcp add` and settings and cost nothing extra.

## Installing

```sh
mcs pack add breferrari/vigia
mcs sync --global
```

`--global` is the scope the README recommends: it registers the server for your user and installs the hooks under `~/.claude/`, so every repository you open is covered.

## What it installs

| Component | What it is |
|---|---|
| `vigia.vigia-cli` | `brew install breferrari/tap/vigia` |
| `vigia.notes-server` | The `vigia mcp` stdio server, registered as `vigia` |
| `vigia.hook-register-start`, `-end` | `vigia mcp register`, on `SessionStart` and `SessionEnd` |
| `vigia.hook-pending` | `vigia mcp pending`, on `UserPromptSubmit` |
| `vigia.notes` | A `CLAUDE.local.md` section on what a resolved note is held to |

Only the server is needed for the pack to be useful, and each hook is selectable under `mcs sync --customize`. The hooks install as scripts under `.claude/hooks/vigia/` rather than as the bare commands the README shows, because a script is what mcs registers and what lets every path exit clean: a machine without `vigia` gets a session that starts normally and says nothing. `mcs doctor` reports what is missing and the command that fixes it, and `mcs pack validate .` checks the manifest after an edit.

## Links

- [MCS CLI](https://github.com/mcs-cli/mcs)
- [Tech pack schema](https://github.com/mcs-cli/mcs/blob/main/docs/techpack-schema.md)
