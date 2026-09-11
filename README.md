<div align="center">

<img src="assets/banner.jpg" alt="vigia: a watchtower sweeping a beam of light across changed lines of code" width="100%">

### The terminal pane your **coding agent** works beside.

*Portuguese: a watchman, the one who keeps watch. At sea, also a porthole.*

[![crates.io](https://img.shields.io/crates/v/vigia?style=for-the-badge&logo=rust&color=39c5cf&labelColor=0d1117)](https://crates.io/crates/vigia) [![downloads](https://img.shields.io/crates/d/vigia?style=for-the-badge&color=3fb950&labelColor=0d1117)](https://crates.io/crates/vigia) [![ci](https://img.shields.io/github/actions/workflow/status/breferrari/vigia/ci.yml?branch=main&style=for-the-badge&label=ci&color=3fb950&labelColor=0d1117)](https://github.com/breferrari/vigia/actions/workflows/ci.yml) [![license](https://img.shields.io/badge/license-MIT-e3b341?style=for-the-badge&labelColor=0d1117)](LICENSE) [![rust](https://img.shields.io/badge/rust-1.89+-f85149?style=for-the-badge&logo=rust&labelColor=0d1117)](https://www.rust-lang.org)

**Your agent writes in one pane. `vigia` watches in the pane beside it, and carries your words back.**

It is a live diff monitor, and that is the half you can see. The other half is a wire: point at a line, say one sentence, and it reaches the agent that wrote it.

</div>

<img src="assets/preview.svg" alt="The vigia interface: a pinned list of changed files, each row carrying a caret, a status letter, a path, a note mark, a change sparkline, a heat strip and line counts, above a syntax highlighted diff whose own file heading repeats the same row, with a note box open under a changed line holding a half-typed sentence for the agent, a scrollbar down its side and a status bar showing key hints, frame time, resident memory and the follow state." width="100%">

---

## 🔭 Why

**This is not a diff viewer you open.** A diff viewer answers *what changed*, once, when you ask. Agentic engineering does not work like that: the agent edits **fast**, **wide**, and while you are reading something else, and the scrollback tells you what it *said* it did rather than what landed.

`vigia` is the surface between the two of you. It watches what actually lands, continuously and without being touched, and it carries a sentence from you back to the agent, anchored to the line you were looking at. One pane writes. One pane watches, and answers.

|  |  |
|---|---|
| 🤖 **For the pane beside the agent** | Zero input. It follows the newest change and scrolls to it on its own |
| ✍️ **Talk back to a line** | Click a line number, type, `Enter`. Your agent gets the file, the line and your words |
| 🕰️ **Look back without leaving** | `B` stands the pane anywhere in the branch's history, and back |
| 🪶 **Cheap enough to leave open for a week** | Zero wakeups while idle. Under 5% memory drift over 24 hours |
| 📐 **Fits half a laptop screen** | Legible at 40 columns, because that is the actual pane you have |
| ⌨️ **Nothing to learn first** | `?` draws every gesture, `m` every setting. `~/.config/vigia/config` decides what it opens as |

> [!NOTE]
> **A monitor, not a reviewer**, and not because it does less. It browses history and carries your notes to the agent. Because of what it *asks*: a reviewer is something you launch, work through and finish. `vigia` is already open, correct before you touch it, and still correct if you never do.

---

## 🚀 Your first five minutes

**1. Install it and point it at a repo.**

```sh
cargo install vigia                          # or brew install breferrari/tap/vigia
vigia                                        # the tree you are in
```

Leave it in the pane beside your agent. There is nothing to configure and nothing to press.

**2. Let it follow.** As your agent writes, files appear in the pinned list at the top and the diff scrolls itself to whatever landed last. The sparkline says *when*, the heat strip says *where in the file*, the counters say *how much*. You are meant to glance, not to read.

**3. Say something about a line.** Click any line number in the diff and a box opens under it. Type one sentence, press `Enter`.

```
  41 +      self.deadline = Instant::now() + DEBOUNCE;
     ✎ should this reset on every event, or just the first?
```

The note goes to the agent anchored to that file and that line. Its answer arrives on a row under yours. This needs [one registration](#notes), once, for every project you will ever open.

**4. Look back when you need to.** `B` opens a list of every place the pane can stand: the live tree, the branch point, or any commit behind it. Pick one and the whole pane, counts and all, moves there. `Esc` and you are back.

**5. Forget the rest.** Press `?` and every gesture is on screen. Press `m` and every setting is. Neither moves a row.

<details>
<summary><b>Other ways to install</b></summary>

<br>

A prebuilt binary, with no toolchain at all:

```sh
# macOS and Linux
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/breferrari/vigia/releases/latest/download/vigia-installer.sh | sh

# Windows
powershell -ExecutionPolicy Bypass -c "irm https://github.com/breferrari/vigia/releases/latest/download/vigia-installer.ps1 | iex"
```

```sh
vigia ~/code/some-repo                       # or any path
vigia --version                              # or -V. It is the only option there is
```

Every [release](https://github.com/breferrari/vigia/releases) carries archives for x86-64 Linux, Intel and Apple-silicon macOS, and x86-64 Windows. The Linux build is statically linked against musl, so it runs on any distribution without matching a system libc. From source needs Rust 1.89 or newer.

**No C toolchain, on any of those paths.** Every dependency is pure Rust, and CI asserts it on each shipped target rather than claiming it: a `cc`, `cmake` or `bindgen` entering the dependency graph fails the build.

</details>

<details>
<summary><b>Upgrading on Windows, and <code>Access is denied. (os error 5)</code></b></summary>

<br>

Windows will not let anything replace a running `.exe`, and the `vigia mcp` you register below runs for as long as each agent session does. So an upgrade fails while any session is open, naming only cargo and a path: nothing in the error connects a diff monitor's upgrade to a coding agent that started hours earlier. `cargo install` and the PowerShell installer both write to the same directory, so both fail this way. Move the running binary aside and the upgrade goes through, with every open session still served:

```powershell
$bin = if ($env:CARGO_HOME) { "$env:CARGO_HOME\bin" } else { "$env:USERPROFILE\.cargo\bin" }
Move-Item "$bin\vigia.exe" "$bin\vigia.exe.old" -Force
cargo install vigia
```

A running process follows its image through a rename, so the servers keep answering and the notes they hold are untouched. **You are not accumulating copies.** The name is always the same one, so an upgrade recycles it rather than adding to it, and `vigia` deletes it the next time one starts and finds nothing holding it, which is the next pane you open or the next session your agent starts. Windows keeps it alive only for as long as a process that began before the upgrade is still running.

**Do not stop the processes instead.** That frees the same path and splits every session that is open: the socket rung goes on delivering your notes, the MCP half does not come back, and the agent reads a note it no longer has the tools to answer. A failed upgrade tells you what is wrong. That does not.

</details>

---
## 👀 Reading the pane

```
   header  │  my-repo · main · current ▾ · 3 changed                   +55 -10
           │
     list  │  ▸ M src/engine/watch.rs   ●  ■■■■■■■■■■■■  __▁▂▆█__   +42    -7
           │    M src/render/frame.rs      ■■■■■■■■■■■■  ________   +11    -3
           │    M Cargo.toml               ■■■■■■■■■■■■  ________    +2    -0
     rule  │  ───────────────────────────────────────────────────────────────
     diff  │    M src/engine/watch.rs   ●  ■■■■■■■■■■■■  __▁▂▆█__   +42    -7
           │    @@ -38,7 +38,9 @@
           │    38    fn coalesce(&mut self, ev: Event) -> Option<Frame> {
           │    39 +      if self.pending.is_empty() {
           │    40 +          self.deadline = Instant::now() + DEBOUNCE;
     rule  │  ──────────────────────────────────────────────────────────────
   status  │  q quit · f follow · m config · ? keys   3.1ms frame  25MiB  follow ▶  1/3
```

The list is **pinned**, so the signals stay on screen while you read the diff under them. Press `r` on a pane of 139 columns or more and it moves *beside* the diff instead, as a left rail, so a path sits against its own numbers rather than across a void that grows with the pane. It costs the diff real width, which is why you ask for it rather than the pane deciding: `r` again puts it back, and below 139 the key does nothing. The pane drawn above is narrower than that, and the stacked layout is what ships at every width.

Press `s` and the diff shows **only the file the caret is on**. Scrolling stops at that file's two ends instead of carrying on past them into the next one, and the scrollbar measures the file rather than the whole changeset, so you are keeping one position in your head instead of two. It is follow's companion: `f` decides which file the pane goes to on its own, `s` decides how much of the rest of the tree your own scrolling reaches once it is there. `n`, `p`, the digits, a click on a listed file and follow itself all still move between files, and `s` again gives the whole diff back.

Press `a` and the pane adds **what is staged**, as a second run beside the first. Two labelled lists share the top panel, unstaged above staged, and every staged row draws its status letter in green so the two never blur together. The mark costs no width, so asking for the staged run never shortens a filename. It exists because an agent that stages its own work used to empty the pane: `vigia` shows the working tree against the index, so a fully staged worktree had nothing to show and said so, which reads exactly like a clean one. A file staged and then edited again appears in both runs, once for each diff, because they are two different changes. `a` again puts it back, and even with it off a blank pane now says where the work went: `no unstaged changes · 3 staged`.

Every file gets the same row in both regions:

| | | Answers |
|---|---|---|
| `▸` | **caret** | 📍 *where you are.* The diff below is inside this file |
| `M` | **kind** | modified, added, deleted, renamed |
| `src/…` | **path** | which file. How brightly it is drawn is how recently it changed, and it is a link you can click |
| `●` | **pulse** | ⚡ it changed on the newest tick |
| `✎` `↳` `✓` | **note mark** | 📝 your note is here, and where it stands |
| green `M` | **staged** | 📦 this row is what the index holds, not the working tree (`a`) |
| `■■■■` | **heat strip** | 🗺️ **where** in the file the change is |
| `__▁▂▆█` | **sparkline** | ⏱️ **when** it changed, over the last two minutes |
| `+42 -7` | **counters** | 📊 **how much**, in lines |

They exist separately because a glance can only ask one question. You read the one you came for and ignore the rest.

<details>
<summary><b>📝 The note mark is <i>whose turn it is</i></b></summary>

`✎` is a note you left that the agent has not answered. `↳` is one it has answered and you have not resolved, and it is the only one of the three that means the pane is waiting on **you**: it draws the same arrow the agent's reply draws under your line. `✓` is one just resolved, and it stays for as long as the note's departure does.

A file with several notes shows the one that matters most, so an answer beats a note still waiting and a resolved one never hides either. The shape is what carries the state, not the colour, so all three still read on a terminal with `NO_COLOR` set. And the slice of the heat strip the note sits in is tinted, so on a long file you can see roughly where in it the conversation is without opening it.

The column is kept on every row whether or not the file has a note, so nothing slides sideways when one arrives. It is the last thing dropped before the counters as the pane narrows, which puts it ahead of the pulse: a file that just changed says so in three other ways, and a file holding a conversation says so in one.

</details>

<details>
<summary><b>🗺️ The heat strip is <i>where</i></b></summary>

<br>

Cut the file into equal slices, top to bottom, twelve of them on an ordinary pane and twenty-four on one wide enough to spare the columns, and colour each one by what happened inside it: **green** for lines added, **red** for removed, and a third colour where both. It is a map of the file you are *not* looking at, so a strip lit only at its right end says the change is at the bottom and you have not scrolled there yet. Brighter means more lines in that slice.

A slice nothing touched is still drawn, in a dark track colour, because a strip with holes in it would be a different shape per file and you could not compare two at a glance. On a narrow pane adjacent slices are summed and reclassified rather than dropped, so it stays a whole file at every width.

</details>

<details>
<summary><b>⏱️ The sparkline is <i>when</i></b></summary>

<br>

Twelve columns across the last two minutes, so each column is ten seconds, oldest on the left. A taller column is more bytes moving around that ten seconds, and a busier one is a hotter colour: on a terminal with 24-bit colour the height and the ink climb the same ramp together, so the shape reads at a glance and the colour confirms it. Every column always covers the whole two minutes between them: a narrow pane draws six columns of twenty seconds rather than the last minute, and a pane wide enough to spare the room draws twenty-four of five.

**Around, rather than in.** A save is a point event, so the raw samples are zero almost everywhere and drawing them gives you a spike train on a flat line rather than a graph. What the column draws is a **level**: the bytes near it, weighted by a six-second kernel that looks both ways, which is what reading a series of point events as a density means. The mockup drew these as waves before the first commit, and drawing the events raw was the defect.

It is scaled **across every tracked file**, not against the row's own maximum, and that is the whole point: a row scaled to itself would draw full height the moment it was the busiest thing *it* had ever been, and you could not tell the file an agent is hammering from the file it touched once. A column with no writes draws a flat track `_` rather than nothing, for the same reason the heat strip draws its empty slices.

</details>

<details>
<summary><b>⚡ The pulse, the caret, and what is <i>not</i> a selection</b></summary>

<br>

The dot marks the file named by the newest tick, and it lasts exactly one tick, so it **cuts rather than fades**. The path's own brightness is the same signal, slower: the file that just changed, one that changed recently, and one that has not, are three intensities of the same colour.

The caret `▸` is a different claim, and the only one about you: the diff below is inside this file. It is a marker, not a cursor. **Nothing on this pane is ever selected**: not the caret, not the row under your pointer. Nothing is remembered because you looked at it, no row becomes special by being pointed at, and the next key means exactly what it would have meant, unless you have a note box open, which is the one thing here you are inside until you leave it. Dragging the diff washes the rows you cross, and that is the exception that proves it: let go, they are on your clipboard, and the wash is gone. A line you left a note on stays marked, and that is not a selection either: you put it there, it outlives the pane, and it goes when the note does.

The counters lend colour only where it says something: a `-0` stays grey, because a zero is not reporting a removal.

</details>

### 🖍️ The diff itself is highlighted, in every language you write

**217 grammars**, so the languages a 2026 tree is actually made of are coloured rather than plain: TypeScript and TSX, Swift, Kotlin, Dart, Elixir, Julia, Zig, Nim, Crystal, F#, Solidity, Gleam, V, Odin, Elm, PowerShell, SCSS and Sass and Less, Vue and Svelte, TOML, Protobuf, GraphQL, Terraform, Dockerfile, CMake, Nix, and `go.mod` and `.gitignore` and `.env`, beside the C-family and scripting languages you would expect.

<details>
<summary><b>🖍️ How a file finds its language, and the four it cannot</b></summary>

<br>

The grammars are `bat`'s curated collection, which is the same set that tool highlights with, compiled into one dump the binary carries. Every one of their licences is reproduced in `NOTICE.md`, which ships in the release archives and in the published crate rather than only living here.

**Five steps decide the language**, in order, because an extension alone gets a surprising number of files wrong:

1. **A written rule**, where one extension has more than one honest answer. `.h` is Objective-C, whose grammar is a superset of C, so C headers colour fully and only C++-only constructs go plain. `.m` is Objective-C over MATLAB, `.v` is V over Verilog, `.jsx` borrows the TSX grammar, and `.sass` is Sass, which it was not: it used to resolve to Ruby Haml, which is a confidently wrong colour rather than a missing one.
2. **The whole file name**, so `Dockerfile`, `CMakeLists.txt` and `go.mod` are found by name. This runs *before* the extension, which is what fixes `CMakeLists.txt`: the CMake grammar registers it whole, and looking up `txt` first handed it to plain text.
3. **The extension**, with a leading-dot retry so `.gitignore` finds a grammar registered as `gitignore`.
4. **The nearest grammar**, for the four formats below.
5. **The first line**, which is how an extensionless script with a `#!` gets a language at all, and how a `.ts` file that is really a Qt translation file gets read as the XML it is instead of as TypeScript.

**Four formats have no grammar this stack can carry**, and they draw as their nearest relative rather than as nothing: `.astro` as HTML and `.bicep` as JavaScript, because both upstreams are written in a Sublime Text 4 dialect `syntect` does not implement and both extend exactly those; `.mdx` as Markdown and `.mojo` as Python, which they are supersets of. Carbon has no grammar anywhere in this format, so it draws plain. That step runs *after* the four above, so the day a real grammar lands it wins without anything being deleted.

A file type nothing recognises is not an error. It draws exactly as it did before there was highlighting at all, because a monitor that refused a file it could not colour would have inverted its own job.

</details>

### 🎯 And *what* changed inside the line

A changed line sits on a calm wash of its own colour, and the words that actually changed sit in a **hotter patch of that same colour**. A renamed function in a long line reads as one bright token rather than a whole red line above a whole green one, and you find the edit without reading either line to its end.

The pairing is bounded on purpose. A removed line and the added line under it are compared token by token, and a pair that differs too much is left as two whole lines rather than confettied into unrelated highlights: past that bound there is no shared shape left to point at. The line numbers of a changed row take a slightly darker tone of the same wash, so the gutter reads as its own column without a border being spent on one.

All three are backgrounds, so they need 24-bit colour and they leave together below it. What is left there is what was always there: the `+` and `-` column, and the left bar beside it.

---

## ⌨️ Drive it

**Keys**

| | |
|---|---|
| `j` `k` `↑` `↓` | scroll a row |
| `Space` `PgDn` `PgUp` | page |
| `d` `u` | half a page |
| `g` `G` | first / last file |
| `n` `p` `→` `←` | next / previous file |
| `1` to `6` | jump to that list row |
| `J` `K` | scroll the pinned list |
| `f` | follow the newest change, or stop |
| `r` | list beside the diff, or above it |
| `s` | one file, or the whole diff |
| `o` | the file list alone, no diff |
| `a` | show or hide staged changes |
| `b` | stand at the branch point |
| `B` | everywhere else it can stand |
| `O` | that commit alone, or everything since it |
| `w` | wrap a long line onto the row below, or clip it |
| `c` | show or hide note rows |
| `Enter` `Esc` | in a box: send, or cancel |
| `m` `Esc` | every setting, in one box |
| `?` `Esc` | all of this, on screen |
| `q` `Ctrl+C` | quit |

**Mouse**

| | |
|---|---|
| wheel | scroll what you point at |
| drag a bar | move that region |
| click a track | send it there |
| click `▲` `▼` | one row, and repeats held |
| click a file | jump the diff to it |
| click the position | everywhere it can stand |
| drag the diff | copy those rows |
| click a line number | open a note there |
| click a note's side | take that note back |
| click `✕` | close the sheet |
| just point | it marks itself |
| `Shift`+drag | your terminal selects text |

**`m` opens every setting in one box**, the ones above and the two only the config file sets, with `↑` `↓` to move and `Space` to flip. `remember between runs` keeps what you flip for next time; `reset to defaults` puts every row back. **`?` draws every gesture on this page**, a page at a time where the pane is small. Both draw over rows that are already there, and `Esc` puts either away.

> [!TIP]
> Press `?` and you never have to remember any of it. The sheet draws over rows that are already there, so **nothing moves** when it opens or closes, and every other key still means what it meant. On a pane too small to hold the whole table, `?` again turns the page and the last one closes it; the title bar says how many of them you are looking at.

<details>
<summary><b>The small print on the keys</b></summary>

<br>

`Ctrl+D` quits too, and `Home` / `End` are aliases for `g` / `G`, `Shift+↑` / `Shift+↓` for `J` / `K`.

**`Shift`+drag selects text because your terminal does it, not because `vigia` does.** The pane holds the mouse so the wheel and the scrollbars work, and every terminal keeps a modifier that hands selection back for as long as you hold it: `Shift` on xterm, GNOME Terminal, Konsole, Windows Terminal, kitty and WezTerm, and **`Option`** on iTerm2, checked by hand on Windows Terminal. You get the terminal's own highlight and the terminal's own clipboard, which means it works over SSH and inside tmux. What it copies is what is on screen, so a path the pane had to shorten is copied short.

**Dragging the diff is for that case.** It sends the rows rather than the cells, so a line the pane clipped arrives whole, a line `w` wrapped arrives as one line, and a file heading arrives as its path however short the pane drew it. Let go and it is sent, and the footer says what went. Your terminal has to allow it: the escape it uses is off by default in a few of them, there is no reply to read, and so nothing can promise it arrived.

The digits count **rows on screen**, not files in the repository: `3` is the third row the list is drawing, so it means a different file once you have scrolled the list with `J`. A digit naming a row that is not drawn does nothing at all, and neither does `n` at the last changed file or `p` at the first.

It shows the working tree against the **index**, untracked files included, and it follows whatever changed last until you scroll away. `a` adds what is *staged* beside it, as a second run, so an agent that stages its own work does not empty the pane. With nothing to show it says so, and says where the work went if it went to the index.

</details>

---

<a id="notes"></a>

## ✍️ Notes to the agent

Point at a line number in the diff and it becomes a pencil `✎`. Click it, type one sentence, press `Enter`.

```
  41 +      self.deadline = Instant::now() + DEBOUNCE;
     ✎ should this reset on every event, or just the first?
```

It goes to the agent in the other pane carrying the file, the line and your words. The answer comes back on a row under yours.

That is the whole of it. `vigia` calls no model, summarises nothing and judges nothing. It carries your words, and the agent answers.

### What a note says while it waits

Every note draws under its line with one word for where it stands.

| | |
|---|---|
| `open` | Sent. The agent has not looked yet |
| `seen` | The agent has read it |
| `changed` | You edited the line underneath, so the row draws dim |
| `gone` | The line left the diff, and the note moved under the file's heading |
| `adrift` | The whole file left the diff. The footer counts it beside the position as `2 notes · 1 adrift` |

**No state loses a note.** An adrift one is back under its line the moment the file returns. A line's number stays lit while a note is on it, and the file's own row in the list carries `✎` while you are waiting and `↳` once the agent has answered, which is how you find one note in a run of thirty files. `c` hides the note rows without hiding those marks.

**When the agent resolves one**, its answer arrives on a row under the note, holds for a minute, and the note leaves.

**To take one back**, point at the note's left side. The cell under your pointer becomes `✕`, and clicking it withdraws the note whether or not the agent has answered. Emptying the box and pressing `Enter` does the same. A note the agent resolved in the meantime is left alone, because its answer is already on its way to you.

**Where they live.** One directory per worktree under your own state directory: `$XDG_STATE_HOME/vigia/`, or `~/.local/state/vigia/`, and `%LOCALAPPDATA%\vigia\state\` on Windows. Never inside the worktree and never inside `.git`, because a monitor that wrote where it watches would wake itself.

### Setting it up

Two pieces, and both go in your own config rather than in the repository, because `vigia` is yours and so is the pane you run it in.

**1. Give the agent the server.** `vigia mcp` is an MCP server over stdio:

```sh
claude mcp add --scope user vigia -- vigia mcp
```

`--scope user` writes `~/.claude.json`, which covers **every project you open on this machine** and stays private to you. Claude Code tells the server which project a session is in, so that one registration finds whichever worktree you are watching, and notes are kept per worktree, so two repositories never see each other's.

The agent gets three tools and one resource:

| | |
|---|---|
| `notes` | Lists what is open, each with its line's current number, the line's text and three lines either side, and marks them `seen` |
| `reply` | Writes a line under a note and leaves it unresolved |
| `resolve` | Closes one. Its line is required, because that line is what you watch arrive |

The resource is `vigia://notes`, and the server announces every change to the store, so an agent that subscribes hears about a note the moment you send it.

**2. Reach the session already running.** With the server alone your note waits until the agent next looks. Three hooks make it arrive instead, and they go in `~/.claude/settings.json`, which is the same scope as the line above: write them once, and every repository is covered.

```json
{
  "hooks": {
    "SessionStart": [
      { "hooks": [{ "type": "command", "command": "vigia mcp register" }] }
    ],
    "SessionEnd": [
      { "hooks": [{ "type": "command", "command": "vigia mcp register" }] }
    ],
    "UserPromptSubmit": [
      { "hooks": [{ "type": "command", "command": "vigia mcp pending" }] }
    ]
  }
}
```

`vigia mcp register` records the session's own socket beside the store when it starts and clears it when it ends. `Enter` then posts the note into that session directly, and a session sitting idle starts a turn on it, so the answer can arrive while you are still looking at the line. The footer says **sent** when a socket took the line, **noted** when a session was registered and none took it, and nothing at all when none is. Nothing is written back, so *sent* is the honest word: it says the line went, never that it arrived.

`vigia mcp pending` is the rung that needs no socket. It puts one line in front of your next prompt saying what your notes are waiting on: a read, a resolve, or you, once the agent has answered one with `reply` and left it with you. Nothing at all when nothing is pending.

<details>
<summary><b>Both steps in one command, if you already use <code>mcs</code></b></summary>

<br>

[`mcs`](https://github.com/mcs-cli/mcs) is a package manager for a Claude Code setup. It is not needed for any of the above and nothing here depends on it. If you already keep your setup that way, `techpack.yaml` at the root of this repository is a pack it can install:

```sh
mcs pack add breferrari/vigia
mcs sync --global
```

**Its `--global` is the scope both steps above already use**, under another name: machine-wide for your user, writing the server into `~/.claude.json` and the hooks into `~/.claude/settings.json`. What it adds is doing both at once, installing the binary with them, and putting back whatever moved when you run `mcs sync` again after an upgrade. `mcs doctor` names anything missing and the command that fixes it.

macOS and Linux only, because the pack installs the binary through Homebrew and its hooks are shell scripts. [`docs/TECHPACK.md`](docs/TECHPACK.md) lists every component and what each one is.

</details>

<details>
<summary><b>Putting the server in the repository instead</b></summary>

<br>

`--scope project` is the one shape here that reaches anybody but you. It writes a `.mcp.json` at the root of the repository, and you commit it, so everyone who clones gets a `vigia` server whether or not they have `vigia` installed:

```json
{
  "mcpServers": {
    "vigia": { "command": "vigia", "args": ["mcp"] }
  }
}
```

Right when the whole team watches its diffs this way, and only then. If it is just you, the user-scoped line above is the one, and it already covers every repository on the machine.

</details>

<details>
<summary><b>The small print on the hooks</b></summary>

<br>

Both commands are safe to install once and forget, which is what makes them worth putting in your user settings at all: outside a repository, with no store, or with nothing open, they do nothing and say nothing, and a hook installed there runs in every project you open.

The socket is Claude Code's own, exported to hooks from v2.1.224, and v2.1.234 on native Windows. Below those the registration finds nothing to record and `Enter` still writes the note to the store, where the server and the `pending` line both reach it.

A user-scoped server is started from your own config directory rather than from the repository, so what tells it where to look is the project Claude Code names for it. That has been dependable since v2.1.238. On anything older it falls back to its working directory, which for a user-scoped server is not your worktree, and `--scope project` is the shape that works there.

`vigia` is not the session's child and nothing comes back down the socket, so whether the session acted on your note, held it behind a permission prompt or dropped it is not something the pane can tell you. And a note whose line has been removed from the diff arrives carrying its anchor alone, since there is no line left to quote.

</details>

---

## ⚡ Promises

Budgets, not hopes. Each one has a test that fails when it is missed, and a regression past any of them **fails the build**.

| | | |
|---|---|---|
| 🔔 | **Event driven** | Zero wakeups while idle. No filesystem event, no work. Never a polling timer |
| 🚀 | **Instant start** | Under 50ms to first paint |
| 🌊 | **Streaming** | First paint under 100ms, even on a 100,000 line diff |
| 🎞️ | **60fps** | Frame time under 16ms at p99, *while files are being written* |
| ♻️ | **Incremental diff** | Re-diff cost scales with what changed, not with your worktree |
| 🖍️ | **Incremental highlight** | Re-parse cost scales with your edit, not the file |
| 🪶 | **Flat over days** | Under 5% memory drift across 24 hours. No retained temp files |
| 🧘 | **Correct untouched** | Follows the newest change and scrolls to it with no input |
| 📐 | **Narrow panes** | Legible at 40 columns |
| 🚪 | **Clean exit** | Terminal restored on every exit it can observe: the quit key, `Ctrl+C`, an error, a panic, or the first kill from outside |

<details>
<summary><b>The numbers behind them</b></summary>

<br>

The scrollbar beside the diff is **row-exact**: it spans the screen's rows over the diff's total rows and sits at the rows above it. Counting every changed file's height turned out to cost **8.76ms** where materialising the same diffs costs **442.71ms**, so the bar says where the end is rather than approximating it. A file the agent is still writing keeps the height it had until the write settles, two seconds after the last one, and is counted once then.

That count is incremental too: a file that has not changed since the last tick is proved unchanged by a `stat` rather than read again, which is **1.29ms against 12.90ms** over a hundred files.

The frame time in the status bar is a promise rather than a diagnostic: it is the p99 of the last 128 frames, against the 16ms this is gated at. The memory beside it is read once a frame and costs about **240ns**, a syscall against a 16ms budget.

</details>

---

## 🎨 Make it yours

A palette, a colour depth, which drawing glyphs your font carries, what the pane opens as, and whether it looks for updates. Every one has a default, and the defaults are what ships.

```sh
VIGIA_THEME=dark vigia          # a built-in palette, or a path to your own
NO_COLOR=1 vigia                # every ladder collapses to something readable
```

**[`docs/CONFIG.md`](docs/CONFIG.md)** is the whole of it: the five ladders and what wins at each rung, the config file and its keys, and how to write a theme. **[`docs/THEME.md`](docs/THEME.md)** names every colour the pane draws.

---

## 🧱 Built with

| | |
|---|---|
| [ratatui](https://github.com/ratatui/ratatui) + [crossterm](https://github.com/crossterm-rs/crossterm) | The TUI, and Windows plus cross-platform mouse |
| [gix](https://github.com/GitoxideLabs/gitoxide) | Pure Rust git. Diffs in process, no subprocess per change |
| [notify](https://github.com/notify-rs/notify) | Native filesystem events, which is what *no polling timer* requires |
| [syntect](https://github.com/trishume/syntect) | Syntax highlighting, pure Rust, so no C toolchain in CI |
| [two-face](https://codeberg.org/CosmicHarper/two-face) | The 217 grammars, [bat](https://github.com/sharkdp/bat)'s curated set |
| [tachyonfx](https://github.com/ratatui/tachyonfx) | Effects over the drawn buffer, so a change can be seen arriving |
| [ratatui-textarea](https://github.com/ratatui/ratatui-textarea) | The note box: its text, its caret, its undo |
| [fancy-regex](https://github.com/fancy-regex/fancy-regex) | The `hide` pattern |

Three of those earn their place by what they *do not* add. `fancy-regex` is already in the graph under `syntect`, so the `hide` pattern costs the binary nothing. `tachyonfx` schedules nothing, which is what keeps *no polling timer* this program's own rule to keep rather than a dependency's. `two-face` builds the grammar dump at compile time and is absent from every shipped graph.

Everything is pure Rust on purpose: a genuinely static Linux binary needs no cross toolchain, and macOS and Windows are plain tier-1 targets.

## 🗺️ Status

`🚧` **Early, and released.** The install lines above are live. The surface is one optional path, `--version`, and the word `mcp` for the notes server, on purpose, and look and feel is where the work is.

| | Phase | |
|---|---|---|
| ✅ | **1. Core engine** | Watch, coalesce, diff, incremental re-diff. No UI |
| ✅ | **2. Minimum monitor** | The TUI: follow mode, scroll, mouse, layout, clean exit |
| ✅ | **3. Glanceability** | Sparklines, heat bars, live counters, the status bar, theming |
| ✅ | **4. The artifacts tell the truth** | README, mockup, spec and tracker agree with each other |
| ✅ | **6. Measured, not assumed** | Claims that outran their evidence get the measurement that settles them |
| ✅ | **7. Distribution** | crates.io, Homebrew tap, prebuilt binaries |
| 🔨 | **8. Look and feel** | Layout, colour, keys, chrome: the polish a first user actually sees |

There is no Phase 5 in that table: the shelf, where deferred work waits with the dated reason it was deferred for, was numbered as one until August and kept its milestone.

Built in the open, spec first. [`SPEC.md`](SPEC.md) is the source of truth and is written *before* the code, so it is the honest place to see where this is going and to argue with it. [`ROADMAP.md`](ROADMAP.md) is the live state, issue linked. [`CHANGELOG.md`](CHANGELOG.md) is every released version and what moved in it.

<details>
<summary><b>🖼️ About that picture at the top</b></summary>

<br>

**It is a mockup, not a screenshot**, and `VIGIA_THEME=dark` is what draws it. All of it draws today: the header with its position token, the blank row under it, the pinned list, the counters in green and red, the sparklines, the heat bars, the caret and the bold path that goes with it, the pulse, the scrollbar with its step buttons, the tinted rows with their left bars and their gutter tones, the highlighted diff, the note box open on a line, and the status bar.

**The picture is a specification here, not decoration.** `SPEC.md` §5.1 rules that where the mockup answers a question the spec left open, the mockup *is* the answer, so every disagreement between it and the binary is either a bug or a departure somebody wrote down. **One is left.** The header reads the worktree's name rather than `vigia`, because a title bar spends six of forty columns telling you which program you started, and what you cannot tell by looking is which tree.

Everything else that disagreed was the picture being behind, and it has been brought forward: the status bar's hints, the position beside the follow marker, the branch, the caret standing on the pane's own edge, the diff's heading drawing the same row as the list above it, and the row's right-hand order, which now places the pulse, heat strip, sparkline and counters where the binary places them. **One more came forward in August 2026**: the sparklines are drawn in the cyan the binary has used since the ramp landed rather than the green they were first mocked in, which is the ruling that green already means *added* two rows down. **Four more in September**: the header's position token and the `▾` that says a list lives behind it, both of which the picture predated; the run's total where the picture used to put the mode word, which was written down as a departure and is not one, since the word draws there only when there is nothing to count and this pane has three files; `m config` back on the status bar, which #80 had cut when there was no config menu for it to name; and the note box, which had never been drawn at all. A picture of a monitor that only ever watches describes half of what this is.

</details>

## 🏷️ The name

*Vigia* is Portuguese: a watchman, a lookout, the one who keeps watch. At sea it also means a porthole, the small round window you look through.

Both readings are the tool. It watches, and it is the window you watch through.

It is also the verb, third person. So `vigia .` reads as a sentence.

---

<div align="center">

**MIT** · Built in the open · [SPEC](SPEC.md) · [ROADMAP](ROADMAP.md) · [CHANGELOG](CHANGELOG.md) · [Issues](https://github.com/breferrari/vigia/issues)

</div>

## 🤝 Contributing

Issues and pull requests are welcome, and a plain bug report needs two lines: what you expected, what happened. [`CONTRIBUTING.md`](CONTRIBUTING.md) has the rest, including the one real ask: `SPEC.md` is read before code.

Five issues are labelled [`good first issue`](https://github.com/breferrari/vigia/labels/good%20first%20issue).
