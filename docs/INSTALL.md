# Installing vigia

Homebrew, on macOS and Linux:

```sh
brew install breferrari/tap/vigia
```

A prebuilt binary, with no toolchain at all:

```sh
# macOS and Linux
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/breferrari/vigia/releases/latest/download/vigia-installer.sh | sh

# Windows
powershell -ExecutionPolicy Bypass -c "irm https://github.com/breferrari/vigia/releases/latest/download/vigia-installer.ps1 | iex"
```

From source, with a Rust toolchain:

```sh
cargo install vigia
```

```sh
vigia ~/code/some-repo                       # or any path
vigia --version                              # or -V. It is the only option there is
```

Every [release](https://github.com/breferrari/vigia/releases) carries archives for x86-64 Linux, Intel and Apple-silicon macOS, and x86-64 Windows. The Linux build is statically linked against musl, so it runs on any distribution without matching a system libc. `cargo install vigia` builds the latest release, which needs the Rust version the badge on the [README](https://github.com/breferrari/vigia#readme) names. Building `main` needs the latest stable Rust.

**Check a download, from 1.2.0 on.** GitHub stores a build attestation for each platform archive in a release, tying it to this repository's release workflow and the commit it built. Download the archive for your platform from the release page and verify it with `gh attestation verify <archive> --repo breferrari/vigia`; an installer fetches the same archive. Each release binary also carries the list of crates it was built from (a `cargo install` build does not), so `cargo audit bin "$(command -v vigia)"` can check an installed copy for known advisories. Archives before 1.2.0 carry neither.

**No C toolchain, on any of those paths.** Every dependency is pure Rust, and CI asserts it on each shipped target rather than claiming it: a `cc`, `cmake` or `bindgen` entering the dependency graph fails the build.

## Upgrading on Windows, and `Access is denied. (os error 5)`

Windows will not let anything replace a running `.exe`, and the `vigia mcp` you register for [notes](NOTES.md) runs for as long as each agent session does. So an upgrade fails while any session is open, naming only cargo and a path: nothing in the error connects a diff monitor's upgrade to a coding agent that started hours earlier. `cargo install` and the PowerShell installer both write to the same directory, so both fail this way. Move the running binary aside and the upgrade goes through, with every open session still served:

```powershell
$bin = if ($env:CARGO_HOME) { "$env:CARGO_HOME\bin" } else { "$env:USERPROFILE\.cargo\bin" }
Move-Item "$bin\vigia.exe" "$bin\vigia.exe.old" -Force
cargo install vigia
```

A running process follows its image through a rename, so the servers keep answering and the notes they hold are untouched. **You are not accumulating copies.** The name is always the same one, so an upgrade recycles it rather than adding to it, and `vigia` deletes it the next time one starts and finds nothing holding it, which is the next pane you open or the next session your agent starts. Windows keeps it alive only for as long as a process that began before the upgrade is still running.

**Do not stop the processes instead.** That frees the same path and splits every session that is open: live delivery into the session keeps working, but the agent's MCP server does not come back, and the agent reads a note it no longer has the tools to answer. A failed upgrade tells you what is wrong. That does not.
