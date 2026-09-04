//! What the published `.crate` carries, and what the release pipeline does.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The two shapes a test uses to read outside this package.
const PATH_ATTRIBUTE: &str = concat!("#[path = \"..", "/../");
const CLIMBING_LITERAL: &str = concat!("\"..", "/..");
/// One level up reaches a sibling crate, which is outside this package just as
/// surely as the repository root is.
const SIBLING_LITERAL: &str = concat!("join(\"..", "\")");

/// How many of `vigia`'s test files read outside the package.
const ESCAPING_FILES: usize = 24;

/// The English spelling of [`ESCAPING_FILES`], which is how the prose says it.
const ESCAPING_FILES_SPELLED: &str = "twenty-four";

/// The repository root, two levels above this package.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repository root is two levels above this package")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Every `.rs` file directly under this package's `tests/`, by file name.
fn test_files() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut found: Vec<(String, String)> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .map(|entry| entry.expect("a readable directory entry").path())
        .filter(|path| path.extension().is_some_and(|e| e == "rs"))
        .map(|path| {
            let name = path
                .file_name()
                .expect("a file with an extension has a name")
                .to_string_lossy()
                .into_owned();
            (name, read(&path))
        })
        .collect();
    found.sort();
    // Twenty-two today.
    assert!(
        found.len() >= 20,
        "found only {} test files, which means the scan is looking in the wrong \
         place and every assertion built on it is vacuous",
        found.len()
    );
    found
}

/// Does this test's source read anything outside the package?
fn escapes(source: &str) -> bool {
    source.contains(PATH_ATTRIBUTE)
        || (source.contains("CARGO_MANIFEST_DIR")
            && (source.contains(CLIMBING_LITERAL) || source.contains(SIBLING_LITERAL)))
}

/// Every test file that reads outside the package, by name, sorted.
fn escaping_tests() -> Vec<String> {
    let escaping: Vec<String> = test_files()
        .into_iter()
        .filter(|(_, source)| escapes(source))
        .map(|(name, _)| name)
        .collect();

    // Exactly this many, not at least.
    assert_eq!(
        escaping.len(),
        ESCAPING_FILES,
        "the three documents that state this count all say {ESCAPING_FILES}. \
         Found {}: {escaping:?}. Update them together",
        escaping.len()
    );
    // This file must be among them, which is the check with the sharpest teeth:
    // `package.rs` reads `SPEC.md` and three workflows, so if the scanner stops seeing
    // it, the scanner is broken rather than the repository clean.
    assert!(
        escaping.iter().any(|name| name == "package.rs"),
        "the scanner no longer detects its own escape, so it has stopped \
         working: {escaping:?}"
    );

    escaping
}

/// The entries of a TOML array declared as `key = [ … ]`.
fn toml_array(source: &str, key: &str) -> Vec<String> {
    // Comments first, on the same rule as `without_comments` for YAML: a `#` anywhere
    // inside a multi-line array would otherwise be split on commas along with
    // everything else and produce entries made of prose.
    let source = without_comments(source);
    let start = source
        .find(&format!("\n{key} = ["))
        .unwrap_or_else(|| panic!("expected a `{key} = [` array in this manifest"));
    let open = source[start..].find('[').expect("the array opens") + start;
    let close = source[open..]
        .find(']')
        .unwrap_or_else(|| panic!("the `{key}` array is never closed"))
        + open;
    source[open + 1..close]
        .split(',')
        .map(|entry| entry.trim().trim_matches('"').to_owned())
        .filter(|entry| !entry.is_empty())
        .collect()
}

/// The shell commands a workflow actually runs, one string per step.
fn run_commands(yaml: &str) -> Vec<String> {
    let lines: Vec<&str> = yaml.lines().collect();
    let mut commands = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim_start();
        // `- run:` as well as `run:`, since the first step of a list carries the dash
        // on the same line as its first key.
        let (rest, indent) = if let Some(rest) = trimmed.strip_prefix("- run:") {
            (rest, line.len() - trimmed.len() + 2)
        } else if let Some(rest) = trimmed.strip_prefix("run:") {
            (rest, line.len() - trimmed.len())
        } else {
            index += 1;
            continue;
        };
        let rest = rest.trim();

        // `|`, `>`, and either with a chomping indicator (`|-`, `|+`, `>-`) or
        // an explicit indentation digit. All are ordinary YAML and `|-` in
        // particular is what a formatter or a careful author writes, so reading
        // only the bare forms turned a legitimate rewrite into a red claiming
        // the file had no publish command.
        let is_block = rest.is_empty()
            || (rest.starts_with(['|', '>'])
                && rest[1..]
                    .chars()
                    .all(|c| matches!(c, '-' | '+' | '0'..='9')));

        // Both arms absorb the continuation lines, and only the block one did at first.
        let mut collected: Vec<String> = if is_block {
            Vec::new()
        } else {
            vec![rest.to_owned()]
        };
        index += 1;
        while index < lines.len() {
            let next = lines[index];
            if next.trim().is_empty() {
                index += 1;
                continue;
            }
            let next_indent = next.len() - next.trim_start().len();
            if next_indent <= indent {
                break;
            }
            collected.push(next.trim().to_owned());
            index += 1;
        }
        let mut body = collected.join("\n");

        // Shell line continuations, so a flag on the next physical line belongs
        // to the same command.
        while let Some(at) = body.find("\\\n") {
            body.replace_range(at..at + 2, " ");
        }
        commands.push(body.split_whitespace().collect::<Vec<_>>().join(" "));
    }

    commands
}

/// Did `cargo package` fail because this machine cannot reach the registry,
/// rather than because the manifest is broken?
fn registry_unreachable(stderr: &str) -> bool {
    const MARKERS: [&str; 7] = [
        "offline mode",
        "failed to fetch",
        "failed to download",
        "network failure",
        "spurious network error",
        "could not connect",
        "could not resolve",
    ];
    // Only the lines cargo failed on.
    let lowered = stderr.to_lowercase();
    lowered
        .lines()
        .filter(|line| !line.trim_start().starts_with("warning:"))
        .any(|line| MARKERS.iter().any(|marker| line.contains(marker)))
}

/// Assert that `preflight` carries a create-then-delete write probe against
/// `repo`, anchored on the shell variable naming the ref it creates.
fn assert_write_probe(preflight: &str, ref_var: &str, repo: &str) {
    // `refs/` rather than `refs/heads/`: the two probes deliberately sit in
    // different namespaces, since only one of them needs to avoid raising a
    // branch event, and the anchor is about which probe this is rather than
    // where it writes.
    let opens = format!("{ref_var}=\"refs/");
    let creates = format!("repos/{repo}/git/refs");
    let undoes = format!("repos/{repo}/git/${{{ref_var}}}");

    let at = preflight
        .find(&opens)
        .unwrap_or_else(|| panic!("no probe names a ref in `{ref_var}`: {preflight}"));
    let created = preflight.find(&creates).unwrap_or_else(|| {
        panic!(
            "the pre-flight must attempt a real write to {repo}. Reading it, or \
             reading a permissions field about it, both pass for a token that \
             cannot push: {preflight}"
        )
    });
    let undone = preflight.find(&undoes).unwrap_or_else(|| {
        panic!(
            "the write probe against {repo} must undo itself, or every run leaves a branch behind"
        )
    });
    assert!(
        at < created && created < undone,
        "the {repo} probe is out of order: it must name the ref, create it, then delete it"
    );

    // The verb, not only the endpoint. Naming the URL alone leaves the probe
    // green after its create is quietly changed to a read, and reading proves
    // nothing here: reading a public repository needs no grant at all.
    assert!(
        preflight[at..created].contains("-X POST"),
        "the probe of {repo} is not a write: {}",
        &preflight[at..created]
    );
    assert!(
        preflight[created..undone].contains("-X DELETE"),
        "the probe of {repo} does not delete the ref it created: {}",
        &preflight[created..undone]
    );
    // And the create is checked, or a 403 passes silently: `curl` exits 0 on
    // one, and the delete that follows would be undoing a ref never made.
    let checked = &preflight[created..undone];
    assert!(
        checked.contains(r#"!= "201""#),
        "nothing checks that the write probe against {repo} succeeded: {checked}"
    );
    assert!(
        checked.contains("exit 1"),
        "the write probe against {repo} notices a failed create and continues \
         anyway, which is a probe that reports rather than guards: {checked}"
    );
}

/// The step named `name`, from its `- name:` to the next step's.
fn step_block<'a>(bump: &'a str, name: &str) -> &'a str {
    let header = format!("- name: {name}");
    let at = bump
        .find(&header)
        .unwrap_or_else(|| panic!("bump.yml has no step named `{name}`"));
    let rest = &bump[at + header.len()..];
    rest.find("\n      - ").map_or(rest, |end| &rest[..end])
}

/// The mapping keys sitting at exactly `indent` columns, unquoted.
fn keys_at_indent(text: &str, indent: usize) -> Vec<String> {
    text.lines()
        .filter(|line| {
            line.len() > indent
                && line[..indent].chars().all(|c| c == ' ')
                && !line[indent..].starts_with(' ')
        })
        .filter_map(|line| line.trim().split_once(':'))
        .map(|(key, _)| {
            key.trim()
                .trim_matches(|c| c == '"' || c == '\'')
                .to_owned()
        })
        .collect()
}

/// A step that must run on every dispatch carries nothing that can skip it.
fn assert_step_always_runs(bump: &str, name: &str) {
    let keys = keys_at_indent(step_block(bump, name), 8);
    for key in ["if", "continue-on-error"] {
        assert!(
            !keys.contains(&key.to_owned()),
            "the `{name}` step carries a `{key}:`, so it can be skipped or its \
             failure ignored while every gate reading its commands stays green"
        );
    }
}

/// The step named `name` runs only when this is not a rehearsal.
fn assert_step_skips_a_rehearsal(bump: &str, name: &str) {
    let block = step_block(bump, name);
    let at = block
        .find("        if:")
        .unwrap_or_else(|| panic!("the `{name}` step has no `if:` at all"));
    let condition = block[at..]
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .trim_start_matches("if:")
        .trim();
    // The whole condition, not a substring of it. `!inputs.rehearse ||
    // github.event_name == 'workflow_dispatch'` contains the needle and is
    // always true, so a `contains` check licenses the exact thing it forbids.
    assert_eq!(
        condition, "${{ !inputs.rehearse }}",
        "the `{name}` step's condition is `{condition}`, which is not simply \
         `not a rehearsal`, so a rehearsal may move the default branch it \
         promises to leave alone"
    );
}

/// One job, carrying nothing that can skip it.
fn assert_one_job_that_always_runs(bump: &str) {
    let jobs = &bump[bump.find("\njobs:").expect("bump.yml defines jobs")..];
    let names = keys_at_indent(jobs, 2);
    assert_eq!(
        names.len(),
        1,
        "bump.yml defines {} jobs ({names:?}); the gates below reason about one, \
         and a second job that nothing `needs:` runs the commit whether or not \
         the tokens were checked",
        names.len()
    );

    let job = keys_at_indent(jobs, 4);
    for key in ["if", "continue-on-error"] {
        assert!(
            !job.contains(&key.to_owned()),
            "the bump job carries a `{key}:`, which skips or forgives every step \
             in it at once, including the pre-flight"
        );
    }
}

/// `first` appears before `second` in the workflow's text.
fn assert_precedes(bump: &str, first: &str, second: &str, why: &str) {
    let at = bump
        .find(first)
        .unwrap_or_else(|| panic!("`{first}` is not in bump.yml at all"));
    let then = bump
        .find(second)
        .unwrap_or_else(|| panic!("`{second}` is not in bump.yml at all"));
    assert!(at < then, "{why}");
}

/// Text with its `#` comments removed, so a `contains` check cannot be
/// satisfied by prose *about* the thing instead of the thing.
fn without_comments(text: &str) -> String {
    text.lines()
        .map(|line| line.split_once('#').map_or(line, |(code, _)| code))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The `exclude` patterns in this package's manifest.
fn exclude_patterns() -> Vec<String> {
    toml_array(
        &read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")),
        "exclude",
    )
}

/// Does `pattern` cover `tests/<name>`?
fn covers(pattern: &str, name: &str) -> bool {
    let target = format!("tests/{name}");
    match pattern.strip_suffix("**") {
        Some(prefix) => target.starts_with(prefix),
        None => {
            assert!(
                !pattern.contains('*'),
                "`{pattern}` uses a glob shape this gate cannot evaluate; teach \
                 `covers` about it rather than letting it answer no"
            );
            pattern == target
        }
    }
}

/// Read a workspace-level file by repository-relative path.
fn repo_file(relative: &str) -> String {
    read(&repo_root().join(relative))
}

/// Every workspace member, as (repository-relative directory, package name).
fn workspace_members() -> Vec<(String, String)> {
    toml_array(&repo_file("Cargo.toml"), "members")
        .into_iter()
        .map(|dir| {
            // Comments stripped first, which is the rule every other parser in this
            // file follows and the one a line-prefix scan needs most: a `# name =
            // "the-old-name"` sitting between `[package]` and the real field is exactly
            // what someone leaves behind while renaming a crate, and `find_map` would
            // take it.
            let manifest = without_comments(&repo_file(&format!("{dir}/Cargo.toml")));
            let package = manifest
                .split_once("[package]")
                .unwrap_or_else(|| panic!("{dir}/Cargo.toml has no [package] section"))
                .1
                .lines()
                .find_map(|line| line.trim().strip_prefix("name = "))
                .map(|name| name.trim().trim_matches('"').to_owned())
                .unwrap_or_else(|| panic!("{dir}/Cargo.toml declares no package name"));
            (dir, package)
        })
        .collect()
}

/// The members `cargo publish --workspace` actually ships: every workspace
/// member whose manifest does not declare `publish = false`.
fn published_members() -> Vec<(String, String)> {
    workspace_members()
        .into_iter()
        .filter(|(dir, _)| {
            !without_comments(&repo_file(&format!("{dir}/Cargo.toml")))
                .split_once("[package]")
                .map(|(_, rest)| rest)
                .unwrap_or_default()
                .lines()
                .take_while(|line| !line.trim_start().starts_with('['))
                .any(|line| line.trim().starts_with("publish = false"))
        })
        .collect()
}

/// Whether a `cargo package --list` names `file` at the package root.
fn listed_has(listed: &str, file: &str) -> bool {
    listed.lines().any(|line| line.trim() == file)
}

/// `cargo package --list` for one member, or `None` if the registry is away.
fn package_list(package: &str, gate: &str) -> Option<String> {
    let output = Command::new(env!("CARGO"))
        .args(["package", "--list", "--allow-dirty", "-p", package])
        .current_dir(repo_root())
        // Colour off, or the classifier below cannot see a `warning:`.
        .env("CARGO_TERM_COLOR", "never")
        .output();

    let output = match output {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                registry_unreachable(&stderr),
                "`cargo package --list -p {package}` failed for a reason that is \
                 not the registry being unreachable, so this is the gate firing \
                 rather than the gate being unavailable:\n{stderr}"
            );
            eprintln!(
                "::warning::SKIPPED {gate}: the registry index is unreachable, so \
                 this gate proved nothing about {package} on this run. \
                 RELEASE-SMOKE.md §1 covers it by hand.\n{stderr}"
            );
            return None;
        }
        Err(e) => panic!("could not run cargo at all: {e}"),
    };

    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Every test that reads outside this package is excluded from the package.
#[test]
fn every_test_that_reads_outside_the_package_is_excluded_from_it() {
    let patterns = exclude_patterns();
    let escaping = escaping_tests();

    for name in &escaping {
        assert!(
            patterns.iter().any(|p| covers(p, name)),
            "tests/{name} reads outside the package but no `exclude` pattern \
             covers it, so a published .crate would carry a test that cannot \
             compile. Patterns are {patterns:?}"
        );
    }
}

/// `SPEC.md` §9 names every test file that escapes, and the naming is checked
/// rather than trusted.
#[test]
fn the_spec_names_every_test_that_escapes_the_package() {
    let spec = repo_file("SPEC.md");
    let bullet = spec
        .lines()
        .find(|line| line.contains("A published `.crate` does not carry"))
        .expect("SPEC.md §9 carries the escape bullet");

    for name in escaping_tests() {
        assert!(
            bullet.contains(&format!("`{name}`")),
            "tests/{name} escapes the package and SPEC.md §9's escape bullet \
             does not name it. That bullet was wrong by a factor of four for \
             two phases for exactly this reason"
        );
    }

    // And every document that states the count states the right one.
    for (path, text, phrase) in [
        (
            "SPEC.md",
            spec.as_str(),
            format!("{ESCAPING_FILES_SPELLED} of `vigia`'s test files"),
        ),
        (
            "crates/vigia/Cargo.toml",
            &read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")),
            format!("counts {ESCAPING_FILES_SPELLED} *files*"),
        ),
        (
            "RELEASE-SMOKE.md",
            &repo_file("RELEASE-SMOKE.md"),
            format!("{ESCAPING_FILES_SPELLED} test files that read"),
        ),
    ] {
        assert!(
            text.contains(&phrase),
            "{path} should say {phrase:?}, matching the {ESCAPING_FILES} \
             counted from disk. All three move together"
        );
    }
}

/// The command scan reads commands and nothing else.
#[test]
fn only_the_commands_a_workflow_runs_are_read_as_commands() {
    let inline = run_commands("    steps:\n      - run: cargo publish --workspace\n");
    assert_eq!(inline, vec!["cargo publish --workspace"]);

    // A block scalar, which is what a step becomes the moment it needs a
    // `set -euo pipefail`. The first version of the scan returned nothing here.
    let block = run_commands(
        "      - name: publish\n        run: |\n          set -euo pipefail\n          cargo publish --workspace --locked\n      - name: next\n",
    );
    assert_eq!(
        block,
        vec!["set -euo pipefail cargo publish --workspace --locked"],
        "a block body is one command, and the step after it is not part of it"
    );

    // A `name:` quoting the command is not the command. The second version of
    // the scan matched this and so passed while the real step did nothing.
    let decoy = run_commands(
        "      - name: publish (was cargo publish --workspace --locked, split for retries)\n        run: echo skip\n",
    );
    assert_eq!(
        decoy,
        vec!["echo skip"],
        "a step's name may say anything; only its `run:` is a command"
    );

    // A continuation, so a flag on the second physical line is still part of
    // the command. `--dry-run` hidden this way passed the gate that exists to
    // reject it.
    let continued = run_commands(
        "        run: |\n          cargo publish --workspace --locked \\\n            --dry-run\n",
    );
    assert_eq!(
        continued,
        vec!["cargo publish --workspace --locked --dry-run"],
        "a `\\` continuation joins, or a flag on the next line is invisible"
    );

    // A continuation after the *inline* form.
    let inline_continued = run_commands(
        "        run: cargo publish --workspace --locked \\\n          --dry-run\n        env:\n          TOKEN: x\n",
    );
    assert_eq!(
        inline_continued,
        vec!["cargo publish --workspace --locked --dry-run"],
        "an inline command continues onto more-indented lines, and stops at the \
         next sibling key"
    );

    // A chomping indicator is still a block scalar. Reading `|-` as the command
    // itself dropped the body and reported the file had no publish in it.
    let chomped = run_commands("        run: |-\n          cargo publish --workspace --locked\n");
    assert_eq!(
        chomped,
        vec!["cargo publish --workspace --locked"],
        "`|-` is ordinary YAML and must read as a block, not as a command"
    );

    // The dash form bounds its body by the *key's* column, not the dash's.
    // Measuring from the dash swallowed the step's sibling keys.
    let dashed = run_commands(
        "      - run: |\n          cargo publish --workspace\n        env:\n          TOKEN: x\n",
    );
    assert_eq!(
        dashed,
        vec!["cargo publish --workspace"],
        "`env:` is a sibling key of `run:`, not part of the command"
    );

    // And the real file yields exactly one command containing a publish.
    let real = run_commands(&without_comments(&repo_file(
        ".github/workflows/publish-crates-io.yml",
    )));
    assert_eq!(
        real.iter()
            .filter(|command| command.contains("cargo publish"))
            .count(),
        1,
        "the real workflow should hold exactly one publish: {real:?}"
    );
}

/// The skip condition is checked against stderr cargo really produced.
#[test]
fn the_unreachable_registry_is_told_apart_from_a_broken_manifest() {
    // Captured: `CARGO_HOME=<empty> CARGO_NET_OFFLINE=true cargo package --list`
    let cold_offline = "error: no matching package named `notify` found\n\
         location searched: crates.io index\n\
         required by package `vigia-core v0.1.0 (…/crates/vigia-core)`\n\
         As a reminder, you're using offline mode (--offline) which can \
         sometimes cause surprising resolution failures, if this error is too \
         confusing you may wish to retry without `--offline`.";
    assert!(
        registry_unreachable(cold_offline),
        "the offline shape must skip, or an offline developer gets a red they \
         cannot act on"
    );

    // Captured: the same command with `readme = "READMEE.md"` in the manifest.
    let broken_manifest = "error: readme `READMEE.md` does not appear to exist (relative to \
         `…/crates/vigia`).\nPlease update the readme setting in the manifest \
         at `…/crates/vigia/Cargo.toml`.";
    assert!(
        !registry_unreachable(broken_manifest),
        "a broken manifest must fire the gate; skipping here is exactly the \
         defect that let a one-character typo turn the package gate green"
    );

    // The capitalised forms, since cargo does not capitalise consistently and
    // the match is case-insensitive for that reason rather than by habit.
    assert!(
        registry_unreachable("Could not connect to server"),
        "cargo capitalises this one mid-sentence"
    );
    assert!(
        registry_unreachable("Caused by: [5] Could not resolve proxy name"),
        "the proxy shape is a resolution failure too"
    );

    // And an unrelated failure is not swallowed.
    assert!(
        !registry_unreachable("error: failed to parse manifest at `Cargo.toml`"),
        "a manifest parse error is the gate firing, not the gate being \
         unavailable"
    );

    // Every marker is exercised, because a marker nobody has ever matched is the thing
    // this whole function was rewritten to stop being.
    for marker in [
        "error: failed to fetch `https://github.com/rust-lang/crates.io-index`",
        "error: failed to download from `https://crates.io/api/v1/crates/gix`",
        "Caused by: network failure seems to have happened",
        "error: spurious network error (3 tries remaining)",
    ] {
        assert!(
            registry_unreachable(marker),
            "{marker:?} should read as an unreachable registry"
        );
    }

    // A *recovered* blip must not vouch for a later, unrelated failure. Cargo
    // prints the warning and carries on, so this transcript ends in a broken
    // manifest and has to fire the gate despite the network line above it.
    let recovered_then_broken = "warning: spurious network error (3 tries remaining)\n\
         error: readme `READMEE.md` does not appear to exist";
    assert!(
        !registry_unreachable(recovered_then_broken),
        "a warning cargo recovered from must not excuse the error it then hit"
    );
}

/// The internal dependency's pinned version is the workspace version.
#[test]
fn the_internal_dependency_tracks_the_workspace_version() {
    let root = repo_file("Cargo.toml");
    let clean = without_comments(&root);

    // Scoped to the `[workspace.package]` block, not the first `version = ` in the
    // file.
    let package_block = clean
        .split("\n[workspace.package]")
        .nth(1)
        .expect("the root manifest declares [workspace.package]");
    let package_block = package_block
        .split_once("\n[")
        .map_or(package_block, |(block, _)| block);

    let workspace_version = package_block
        .lines()
        .find_map(|line| line.trim().strip_prefix("version = "))
        .map(|value| value.trim().trim_matches('"').to_owned())
        .expect("[workspace.package] declares a version");

    // Taken to the next `"` rather than to the end of the line, because the keys
    // inside an inline table have no required order: writing
    // `{ version = "0.1.0", path = … }` is equally valid TOML and the
    // end-of-line form read it as `0.1.0", path = "crates/vigia-core`, which
    // reports a stale pin that is not stale.
    let pinned = clean
        .lines()
        .find(|line| line.trim_start().starts_with("vigia-core = {"))
        .and_then(|line| line.split_once("version = \""))
        .and_then(|(_, rest)| rest.split_once('"'))
        .map(|(version, _)| version.to_owned())
        .expect("[workspace.dependencies] pins vigia-core by version");

    assert_eq!(
        pinned, workspace_version,
        "vigia-core is pinned at {pinned} while the workspace is {workspace_version}; \
         `cargo publish --workspace` would ship a vigia that depends on an older \
         vigia-core, permanently and without anything else going red"
    );

    // Non-vacuity: both reads must have found a real version rather than an
    // empty string, which would compare equal and prove nothing.
    assert!(
        workspace_version.contains('.'),
        "parsed {workspace_version:?} as the workspace version, which is not one"
    );

    // And the shell must still *use* the workspace entry.
    let shell = read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"));
    assert!(
        without_comments(&shell).contains("vigia-core.workspace = true"),
        "crates/vigia/Cargo.toml no longer takes vigia-core from \
         [workspace.dependencies], so the pin checked above is not the one \
         being published"
    );
}

/// Nothing in `exclude` has quietly stopped matching anything.
#[test]
fn nothing_excluded_has_since_stopped_existing() {
    let names: Vec<String> = test_files().into_iter().map(|(name, _)| name).collect();

    // Both arms of `covers` are exercised here rather than left to whatever `exclude`
    // happens to hold.
    assert!(covers("tests/**", "soak.rs"), "the glob arm covers a test");
    assert!(
        covers("tests/soak.rs", "soak.rs"),
        "the literal arm matches"
    );
    assert!(
        !covers("tests/soak.rs", "cli.rs"),
        "the literal arm is exact"
    );
    assert!(!covers("benches/**", "soak.rs"), "a glob elsewhere misses");

    // And the third arm, the one that refuses to guess.
    let unknown = std::panic::catch_unwind(|| covers("tests/*.rs", "soak.rs"));
    assert!(
        unknown.is_err(),
        "a mid-string glob must be refused loudly, not answered `no`, or a \
         pattern this gate cannot evaluate reads as an escape it has caught"
    );

    // Only patterns aimed at `tests/` are checked against test names.
    for pattern in exclude_patterns()
        .into_iter()
        .filter(|pattern| pattern.starts_with("tests/"))
    {
        assert!(
            names.iter().any(|name| covers(&pattern, name)),
            "the `exclude` pattern `{pattern}` matches nothing under tests/, so \
             it is protecting nothing. Either the files moved or the pattern is \
             a leftover"
        );
    }
}

/// The profile the release is built with is the profile the budgets are measured
/// against.
#[test]
fn the_profile_that_ships_is_the_profile_the_budgets_measure() {
    let root = repo_file("Cargo.toml");
    let start = root
        .find("\n[profile.dist]")
        .expect("the workspace declares [profile.dist]");
    let block = &root[start + 1..];
    let block = block
        .find("\n[")
        .map_or(block, |end| &block[..end])
        .trim_end();

    let settings: Vec<&str> = block
        .lines()
        .skip(1)
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();

    assert_eq!(
        settings,
        vec!["inherits = \"release\""],
        "[profile.dist] must inherit `release` and add nothing, or the budgets \
         stop describing the shipped binary"
    );

    // The scan above ends the block at the next `\n[`, so a sub-table like
    // `[profile.dist.package."gix"]` would sit *outside* what it read and tune the
    // release build without this assertion ever seeing it.
    assert!(
        !root.contains("\n[profile.dist."),
        "a [profile.dist.*] sub-table overrides the shipped profile where the \
         block scan above cannot see it; the budgets would stop describing the \
         binary and this gate would stay green"
    );
}

/// The release pipeline actually sends both crates to the registry.
#[test]
fn the_release_pipeline_publishes_to_the_registry() {
    const JOB: &str = "./publish-crates-io";
    const WORKFLOW: &str = ".github/workflows/publish-crates-io.yml";

    let jobs = toml_array(&repo_file("Cargo.toml"), "publish-jobs");
    assert!(
        jobs.iter().any(|entry| entry == JOB),
        "[workspace.metadata.dist] publish-jobs must name {JOB}, or nothing \
         publishes to crates.io on a tag. It holds {jobs:?}"
    );

    // Comment-stripped, because this repository comments heavily and the paragraph
    // above the `run:` line in that workflow quotes the very command asserted here.
    let job = without_comments(&repo_file(WORKFLOW));
    assert!(
        job.contains("workflow_call"),
        "{WORKFLOW} must be a reusable workflow; dist calls it with `uses:`"
    );

    // Only a `run:` body counts as publishing.
    let commands = run_commands(&job);
    let publishes: Vec<&String> = commands
        .iter()
        .filter(|command| command.contains("cargo publish"))
        .collect();

    assert_eq!(
        publishes.len(),
        1,
        "{WORKFLOW} must run exactly one `cargo publish`, found {publishes:?} \
         among {commands:?}"
    );
    let publish = publishes[0];
    assert!(
        publish.contains("--workspace"),
        "{publish:?} must publish with `--workspace`, which orders the two \
         crates and waits for the index; sequential `-p` calls need a sleep of \
         no knowable length"
    );
    assert!(
        publish.contains("--locked"),
        "{publish:?} must publish with `--locked`, or the published crates are \
         built against whatever the registry offers on the day rather than the \
         resolution this repository tested"
    );

    // The one flag that makes this whole job a no-op.
    assert!(
        !publish.contains("--dry-run"),
        "{publish:?} carries --dry-run, so the release would do everything \
         except the publish and report success"
    );

    // `--workspace` publishes every member that does not opt out, so what the workspace
    // *holds* is part of what the release ships.
    let members = toml_array(&repo_file("Cargo.toml"), "members");
    assert_eq!(
        members,
        vec!["crates/vigia-core", "crates/vigia", "xtask"],
        "the workspace member list moved. `cargo publish --workspace` ships \
         every member that does not carry `publish = false`, and a published \
         name is claimed forever, so decide the new list here rather than at \
         the tag"
    );
    assert_eq!(
        published_members().len(),
        2,
        "the set of *published* members moved; the engine/shell split is two \
         crates, and a third publish is a decision, not a side effect"
    );

    // dist generates release.yml from the config above, so this asserts the generated
    // file is current rather than that the config is right.
    let release = without_comments(&repo_file(".github/workflows/release.yml"));
    assert!(
        release.contains("uses: ./.github/workflows/publish-crates-io.yml"),
        "release.yml does not call the registry job; run `dist generate`"
    );
    assert!(
        release.contains("- custom-publish-crates-io"),
        "release.yml must schedule the registry job at all; run `dist generate`"
    );

    // The mechanism is asserted rather than the conclusion, because the obvious
    // ordering claim is false: `announce` waiting on the publish job does not stop a
    // release half-shipping.
    let gh_release_line = release
        .lines()
        .find(|line| line.contains("gh release create"))
        .expect(
            "release.yml no longer creates the GitHub release where this gate \
             expects it, so the ordering documented in publish-crates-io.yml \
             and SPEC.md §9 needs re-reading against the new generated file",
        );
    assert!(
        !gh_release_line.contains("--draft"),
        "the release is created as a draft now, so it is *not* public before \
         the registry job and the documented cost of that ordering is wrong in \
         four places. Better behaviour, but it has to be written down \
         deliberately rather than become true by accident: {gh_release_line}"
    );
    // The flag can also arrive through a variable, which is exactly how the prerelease
    // flag on that same line is passed, so reading the literal alone is defeated by
    // dist's own idiom.
    assert!(
        !release.to_uppercase().contains("DRAFT"),
        "release.yml mentions a draft somewhere, and the ordering documented \
         in publish-crates-io.yml and SPEC.md §9 assumes the GitHub release is \
         public the moment `host` finishes"
    );

    // And the registry job genuinely waits for the artifacts, which is the
    // half of the ordering that is load bearing: publishing a crate before its
    // binaries built would spend the irreversible step on a release that may
    // still fail.
    let registry_job = release
        .split_once("custom-publish-crates-io:")
        .map(|(_, rest)| rest)
        .expect("dist names the custom publish job after its workflow");
    let needs = registry_job
        .split_once("uses:")
        .map(|(needs, _)| needs)
        .expect("a custom job is scheduled with `uses:`");
    assert!(
        needs.contains("host"),
        "the registry job no longer waits on `host`, so it could publish \
         permanently before the artifacts exist: {needs}"
    );
}

/// The no-C-toolchain gate reads its target list from the release config rather
/// than from a second hand-typed copy.
#[test]
fn the_purity_gate_derives_its_targets_from_the_release_config() {
    // Comment-stripped before slicing, because the region is heavily commented and
    // those comments discuss targets by name.
    let ci = without_comments(&repo_file(".github/workflows/ci.yml"));
    // Bounded at both ends.
    let job = ci
        .split_once("assert no cc, cmake or bindgen")
        .map(|(_, rest)| rest)
        .expect("ci.yml carries the no-C-toolchain assertion");
    let job = job
        .split_once("exit $status")
        .map(|(script, _)| script)
        .expect("the no-C-toolchain script ends by exiting its status");

    assert!(
        job.contains("metadata.dist.targets"),
        "the no-C-toolchain job must derive its targets from \
         [workspace.metadata.dist] via `cargo metadata`, not re-type them. A \
         hand-typed second copy is how `x86_64-apple-darwin` shipped unchecked"
    );

    // Non-vacuity: if the job re-acquired a literal list, the derivation above
    // could still be present and unused. No triple should be spelled out.
    for target in toml_array(&repo_file("Cargo.toml"), "targets") {
        assert!(
            !job.contains(&target),
            "{target} is spelled literally in the purity job, which means the \
             hand-typed list is back alongside the derivation"
        );
    }
}

/// The button that cuts a release reaches the workflow that performs one.
#[test]
fn the_release_button_reaches_the_release() {
    let root = repo_file("Cargo.toml");
    assert!(
        without_comments(&root).contains("dispatch-releases = true"),
        "bump.yml starts the release by dispatching it, so the release must be \
         dispatchable. Without this setting dist generates a tag-push trigger \
         and the button bumps the version and releases nothing"
    );

    let release = without_comments(&repo_file(".github/workflows/release.yml"));
    assert!(
        release.contains("workflow_dispatch:"),
        "release.yml has no workflow_dispatch to receive the dispatch; run \
         `dist generate`"
    );

    // The bump must name the workflow that exists, by file name, since `gh
    // workflow run` takes one and a typo there is a 404 at release time.
    let bump = without_comments(&repo_file(".github/workflows/bump.yml"));
    let commands = run_commands(&bump);
    let dispatch = commands
        .iter()
        .find(|command| command.contains("gh workflow run"))
        .expect("bump.yml dispatches the release");

    // The word immediately after `gh workflow run`, not a mention anywhere in
    // the step. A `run: |` body is one string here, and that body ends with an
    // `echo` naming `release.yml` for the log. Asserting `contains` over the
    // whole block therefore passed against `gh workflow run releases.yml`,
    // matching the echo while the dispatch went to a workflow that does not
    // exist, which is a release that silently never happens. Caught by mutation,
    // and it is the third time in this file that a mention has stood in for the
    // thing.
    let target = dispatch
        .split_whitespace()
        .skip_while(|word| *word != "run")
        .nth(1)
        .expect("`gh workflow run` names a workflow");
    assert_eq!(
        target, "release.yml",
        "the bump dispatches {target}, which is not the release workflow"
    );
    assert!(
        dispatch.contains("-f tag="),
        "release.yml's dispatch takes a `tag` input and treats its absence as \
         nothing to do: {dispatch}"
    );

    // And the rehearsal word is dist's, not ours. `release.yml` decides whether
    // to publish by comparing the tag against the literal `dry-run`, so a bump
    // that spelled it differently would publish during a rehearsal.
    assert!(
        bump.contains("dry-run"),
        "the rehearsal path must pass the tag dist reads as build-but-do-not-\
         publish, which is the literal `dry-run`"
    );
    assert!(
        release.contains("'dry-run'"),
        "release.yml no longer compares the tag against `dry-run`, so the \
         rehearsal in bump.yml may now publish"
    );

    // All three tokens are checked before the version moves, and the two that must
    // write are checked for `push`.
    let preflight = commands
        .iter()
        .find(|command| command.contains(r#"-z "${CARGO_REGISTRY_TOKEN"#))
        .expect(
            "bump.yml must guard on the registry token being empty before it              bumps anything. Finding the *name* is not enough: it appears in              this step's own error message and in its `env:` block, so a              search for it passes against a check that has been disabled",
        );
    // Each assertion names a form only the *check* has, never one an error message or
    // an `env:` entry also carries.
    assert!(
        preflight.contains(r#"-z "${HOMEBREW_TAP_TOKEN"#),
        "the pre-flight does not check the tap token is set, and that is the one that failed a release: {preflight}"
    );
    // An actual write, not a report about one.
    assert_write_probe(preflight, "probe", "${tap}");

    // And it has to run before the commit, or it reports a problem the version
    // has already moved past.
    assert_precedes(
        &bump,
        r#"-z "${CARGO_REGISTRY_TOKEN"#,
        "git commit",
        "the token pre-flight runs after the commit, so a bad token would be \
         found only once main already carries the new version",
    );
}

/// The push that moves `main` carries a token that can, and it is checked first.
#[test]
fn the_push_that_moves_main_is_authorised_before_the_version_does() {
    let bump = without_comments(&repo_file(".github/workflows/bump.yml"));
    let commands = run_commands(&bump);

    let preflight = commands
        .iter()
        .find(|command| command.contains(r#"-z "${RELEASE_TOKEN"#))
        .expect(
            "bump.yml must guard on the push token being empty before it bumps \
             anything, in the form only the check has",
        );

    // A real write to *this* repository, told apart from the tap's probe by the
    // repository it names.
    assert_write_probe(preflight, "mine", "${GITHUB_REPOSITORY}");

    // The other half of the bypass, because the write probe does not imply it.
    let admin = preflight
        .find(".permissions.admin")
        .expect("nothing reads the standing of the account behind RELEASE_TOKEN");
    let stops = preflight[admin..]
        .find("::notice::")
        .map_or(preflight.len(), |end| admin + end);
    let standing = &preflight[admin..stops];
    assert!(
        standing.contains(r#"!= "true""#) && standing.contains("exit 1"),
        "the admin standing is read and not acted on, so a token whose owner \
         cannot bypass the required checks passes this step: {standing}"
    );

    // And none of it can be skipped out from under the commit: not the step,
    // not the job it sits in, and not by moving it into a job nothing waits for.
    assert_one_job_that_always_runs(&bump);
    for always in [
        "this runs on main or not at all",
        "the tokens can do what the release will ask of them",
        "hand off to the release",
    ] {
        assert_step_always_runs(&bump, always);
    }

    // And the rehearsal still leaves the default branch alone, which is the one
    // promise the whole rehearsal path makes.
    assert_step_skips_a_rehearsal(&bump, "commit the bump");

    // No step anywhere forgives its own failure.
    assert!(
        !keys_at_indent(&bump, 8).contains(&"continue-on-error".to_owned()),
        "a step in bump.yml carries `continue-on-error:`, so the release can \
         proceed past a step that failed"
    );

    // The commit happens before the dispatch. Reordering them dispatches a
    // release for a version the default branch does not carry yet, which
    // publishes the old code under the new number.
    assert_precedes(
        &bump,
        "git commit",
        "gh workflow run",
        "bump.yml dispatches the release before it commits the version, so the \
         release would build whatever the default branch carried beforehand",
    );

    // A rehearsal dispatches `dry-run` and a real release does not, and the polarity is
    // the assertion.
    let dispatch = commands
        .iter()
        .find(|command| command.contains("gh workflow run"))
        .expect("bump.yml dispatches the release");
    assert!(
        dispatch.contains(r#"= "true" ]; then tag=dry-run"#),
        "the rehearsal's tag no longer depends on `rehearse` being true in the \
         way this gate can read. A flipped comparison here publishes for real \
         on every rehearsal: {dispatch}"
    );

    // The checkout persists no credentials, and this is the assertion the push
    // assertion above rests on.
    assert!(
        bump.contains("persist-credentials: false"),
        "the checkout persists the workflow's token into .git/config, where git \
         prefers it over the token in the push URL: the push would authenticate \
         as the bot and be rejected, with this whole pre-flight passing"
    );

    // The remote the push names carries the token. This is the assertion the
    // whole gate exists for: `git push origin` is what failed, it is one word
    // away from what ships, and every check above passes with it restored.
    let commit = commands
        .iter()
        .find(|command| command.contains("git commit -m"))
        .expect("bump.yml commits the version it raised");
    let remote = commit
        .split("git push")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .expect("bump.yml pushes the commit it made");
    assert!(
        remote.contains("RELEASE_TOKEN"),
        "the bump pushes to `{remote}`, which carries the workflow's own token. \
         That push is rejected by branch protection and cannot ever be accepted, \
         because a GITHUB_TOKEN push triggers no workflow and so can never carry \
         the checks main requires"
    );

    // And it pushes the bump at the default branch.
    let refspec = commit
        .split("git push")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().nth(1))
        .expect("the push names what it is pushing");
    assert!(
        refspec.contains("HEAD:") && refspec.contains("DEFAULT"),
        "the bump pushes `{refspec}` rather than HEAD at the default branch, so \
         the version would land where nothing releases it"
    );

    // And the whole check runs before the commit, on the same reasoning as the
    // gate above it: found afterwards, the version has already moved.
    assert_precedes(
        &bump,
        r#"-z "${RELEASE_TOKEN"#,
        "git commit",
        "the push token is checked after the commit, so a token that cannot \
         move main would be found only once the version had already moved",
    );
}

/// The tarball cargo would actually upload carries no tests.
#[test]
fn the_packaged_artifact_carries_no_tests() {
    let Some(listed) = package_list("vigia", "the_packaged_artifact_carries_no_tests") else {
        return;
    };

    let tests: Vec<&str> = listed
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("tests/"))
        .collect();

    assert!(
        tests.is_empty(),
        "the .crate would carry {tests:?}, and every one of them reaches for \
         something the tarball does not contain"
    );

    // The other direction, so the assertion above cannot pass because the list came
    // back empty.
    assert!(
        listed.contains("src/main.rs"),
        "the package list has no src/main.rs in it, so it is not the list this \
         gate thinks it is reading:\n{listed}"
    );
    assert!(
        listed_has(&listed, "README.md"),
        "README.md is not in the package; `readme` inheritance from the \
         workspace has stopped resolving:\n{listed}"
    );
}

/// Every published crate carries the licence text, not only its SPDX name.
#[test]
fn every_published_crate_ships_the_licence() {
    for (dir, package) in published_members() {
        let Some(listed) = package_list(&package, "every_published_crate_ships_the_licence") else {
            return;
        };

        assert!(
            listed_has(&listed, "LICENSE"),
            "the .crate for {package} carries no LICENSE, so `cargo install` and \
             a vendored copy get the SPDX name with none of the text behind it. \
             `{dir}/LICENSE` is what puts it there, because cargo takes a licence \
             from the package directory and nowhere else:\n{listed}"
        );

        // The other direction, so the assertion above cannot pass because the
        // list came back empty or came from a package this gate did not mean.
        assert!(
            listed_has(&listed, "README.md"),
            "the package list for {package} has no README.md in it, so it is not \
             the list this gate thinks it is reading:\n{listed}"
        );

        // And the grammar dump, for the engine.
        if package == "vigia-core" {
            assert!(
                listed_has(&listed, "assets/syntaxes.bin"),
                "the .crate for {package} carries no assets/syntaxes.bin, and \
                 `Highlighter::new` embeds it with `include_bytes!`, so the \
                 published crate would not build at all:\n{listed}"
            );
            // And the roster, for the same reason one level over.
            assert!(
                listed_has(&listed, "assets/GRAMMARS.txt"),
                "the .crate for {package} carries no assets/GRAMMARS.txt, and \
                 its published test suite embeds it with `include_str!`:\n{listed}"
            );
            // And the attribution, which is the one whose reason lives outside this
            // repository.
            assert!(
                listed_has(&listed, "assets/NOTICE.md"),
                "the .crate for {package} carries no assets/NOTICE.md, so it \
                 ships a few hundred vendored grammars with none of their \
                 attribution:\n{listed}"
            );
        }
    }
}

/// The licence each crate ships is this repository's, byte for byte.
#[test]
fn the_licence_each_crate_ships_is_the_repository_licence() {
    let root = repo_file("LICENSE");

    // Non-vacuity, on the same rule as the package lists above: an empty or
    // truncated root file would make every comparison below pass by matching
    // nothing against nothing, and three empty files are byte-identical.
    assert!(
        root.contains("MIT License") && root.lines().count() > 15,
        "the repository LICENSE is not the text this gate thinks it is \
         comparing against, so the comparisons below prove nothing"
    );

    for (dir, package) in published_members() {
        let shipped = repo_file(&format!("{dir}/LICENSE"));
        assert_eq!(
            shipped, root,
            "{dir}/LICENSE has drifted from the repository LICENSE, so {package} \
             would publish terms that are not this project's. It is a copy \
             because cargo takes a licence from the package directory only, and \
             this is the gate that stops a copy becoming a second licence"
        );
    }
}

/// The grammar count the README states is the one the dump holds.
#[test]
fn the_readme_states_the_grammar_count_the_dump_holds() {
    let roster = repo_file("crates/vigia-core/assets/GRAMMARS.txt");
    let held = roster
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();

    let readme = repo_file("README.md");
    let claim = format!("**{held} grammars**");
    assert!(
        readme.contains(&claim),
        "README.md does not say {claim:?}, and the dump holds {held} grammars. \
         Whichever moved, they have to move together"
    );
}

/// Drives the version raise against `manifest`, in a scratch directory of its
/// own. Returns whether it passed and the manifest it left behind.
#[cfg(unix)]
fn raise_version(case: &str, next: &str, manifest: &str) -> (bool, String) {
    let script = repo_root().join(".github/scripts/raise-version.sh");
    let dir =
        std::env::temp_dir().join(format!("vigia-raise-version-{}-{case}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("create {}: {e}", dir.display()));
    let path = dir.join("Cargo.toml");
    std::fs::write(&path, manifest).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));

    let passed = Command::new("sh")
        .arg(&script)
        .arg(next)
        .arg(&path)
        .output()
        .unwrap_or_else(|e| panic!("run {}: {e}", script.display()))
        .status
        .success();
    let left = read(&path);
    let _ = std::fs::remove_dir_all(&dir);
    (passed, left)
}

/// The raise moves both version strings and counts only the lines it moved.
#[cfg(unix)]
#[test]
fn the_version_raise_counts_the_lines_it_moved_and_nothing_else() {
    // The shape that refused a correct release: the manifest also pins the
    // release tool, and a minor landed on the tool's own number.
    let pinned = "[workspace.package]\n\
                  version = \"0.31.1\"\n\
                  \n\
                  [workspace.dependencies]\n\
                  vigia-core = { path = \"crates/vigia-core\", version = \"0.31.1\" }\n\
                  \n\
                  [workspace.metadata.dist]\n\
                  cargo-dist-version = \"0.32.0\"\n";
    let (passed, left) = raise_version("pinned", "0.32.0", pinned);
    assert!(
        passed,
        "a version equal to a pinned tool's is refused, and the release with it:\n{left}"
    );
    assert!(
        left.contains("\nversion = \"0.32.0\"\n")
            && left.contains("vigia-core = { path = \"crates/vigia-core\", version = \"0.32.0\" }")
            && left.contains("cargo-dist-version = \"0.32.0\""),
        "the raise moved the wrong lines:\n{left}"
    );

    // The real manifest, so the patterns are known to match the lines they
    // claim to, and to touch nothing else in it.
    let before = repo_file("Cargo.toml");
    let (passed, after) = raise_version("real", "9.9.9", &before);
    assert!(
        passed,
        "the raise refuses the repository's own manifest:\n{after}"
    );
    let moved: Vec<&str> = before
        .lines()
        .zip(after.lines())
        .filter(|(was, is)| was != is)
        .map(|(_, is)| is)
        .collect();
    assert_eq!(
        moved,
        [
            "version = \"9.9.9\"",
            "vigia-core = { path = \"crates/vigia-core\", version = \"9.9.9\" }",
        ],
        "the raise changed lines other than the two it is for"
    );

    // And the check still fails the case it exists for: a line the pattern does
    // not match stays at the old number, and the raise says so instead of
    // handing a half-moved manifest to the commit.
    let unmoved = "[workspace.package]\n\
                   version = \"0.31.1\"\n\
                   \n\
                   [workspace.dependencies]\n\
                   vigia-core = { version = \"0.31.1\", path = \"crates/vigia-core\" }\n";
    let (passed, left) = raise_version("unmoved", "0.32.0", unmoved);
    assert!(
        !passed,
        "a dependency line the pattern cannot move passed the check:\n{left}"
    );
}

/// Drives the judgement `ci complete` runs, with fabricated leg results.
#[cfg(unix)]
fn ci_complete(draft: &str, legs: &[&str]) -> bool {
    let script = repo_root().join(".github/scripts/ci-complete.sh");
    std::process::Command::new("sh")
        .arg(&script)
        .arg(draft)
        .args(legs)
        .output()
        .unwrap_or_else(|e| panic!("run {}: {e}", script.display()))
        .status
        .success()
}

/// A draft's skipped legs are not a failure, and everything else still is.
#[cfg(unix)]
#[test]
fn ci_complete_passes_a_draft_that_skipped_everything_and_nothing_else() {
    const OK: [&str; 5] = ["success"; 5];

    assert!(ci_complete("false", &OK), "a full green run has to pass");
    // A push carries no pull request, so the workflow passes an empty draft
    // flag. Reading that as "not a draft" is the whole of it, and reading it as
    // an error turned every push to `main` red.
    assert!(
        ci_complete("", &OK),
        "a push has no pull request and its draft flag is empty, which is not an error"
    );
    assert!(
        ci_complete("true", &["skipped"; 5]),
        "a draft skips every leg by design and the matrix runs on ready_for_review"
    );

    for (draft, legs, why) in [
        (
            "false",
            ["success", "skipped", "success", "success", "success"],
            "a leg that skipped on a ready PR is the absent matrix this gate exists for",
        ),
        (
            "true",
            ["skipped", "success", "skipped", "skipped", "skipped"],
            "a draft that ran some legs and skipped others is a partial run",
        ),
        (
            "false",
            ["success", "failure", "success", "success", "success"],
            "a failing leg fails the gate",
        ),
        (
            "true",
            ["skipped", "failure", "skipped", "skipped", "skipped"],
            "a draft cannot launder a failing leg through its skips",
        ),
        (
            "false",
            ["success", "cancelled", "success", "success", "success"],
            "a cancelled leg never reported and is not a pass",
        ),
        (
            "",
            ["skipped", "skipped", "skipped", "skipped", "skipped"],
            "a push is never a draft, so legs that all skipped on one did not run and should have",
        ),
        (
            "",
            ["success", "failure", "success", "success", "success"],
            "a failing leg on a push fails the gate like any other",
        ),
    ] {
        assert!(!ci_complete(draft, &legs), "{why}");
    }

    assert!(
        !ci_complete("false", &[]),
        "no results at all means the workflow stopped passing them, not that every leg passed"
    );
}

/// The workflow calls the script the gate above proves.
#[test]
fn the_ci_workflow_runs_the_script_the_gate_proves() {
    let ci = read(&repo_root().join(".github/workflows/ci.yml"));
    assert!(
        ci.contains(".github/scripts/ci-complete.sh"),
        "ci.yml does not run ci-complete.sh, so its judgement is gated in a test and unused in CI"
    );
    // The gate above drives the script with whatever a test passes. This pins
    // the shape production actually sends, which is where it broke: on a push
    // the expression below expands to nothing at all.
    assert!(
        ci.contains("github.event.pull_request.draft }}'"),
        "ci.yml does not pass the draft expression to the script, so the gate's draft \
         cases are testing an argument production never sends"
    );
    for leg in ["lint", "test", "benches", "pure-rust", "musl"] {
        let arg = format!("needs.{leg}.result");
        assert!(
            ci.contains(&arg),
            "ci.yml does not pass {arg} to the script, so that leg is judged by nothing"
        );
    }

    // The gate that drives the script is Unix only, which is sound exactly
    // while the job stays on a Unix runner. Asserted here because this test
    // runs on every platform and that one does not.
    let (_, after) = ci
        .split_once("  ci-complete:")
        .expect("ci.yml declares the ci-complete job");
    let runs_on = after
        .lines()
        .find_map(|l| l.trim().strip_prefix("runs-on:"))
        .map(str::trim)
        .expect("ci-complete declares a runner");
    assert_eq!(
        runs_on, "ubuntu-latest",
        "ci-complete moved to {runs_on:?}, so the Unix-only gate over its script \
         no longer covers where it runs"
    );
}

/// The release raises the version through the script the gate above proves.
#[test]
fn the_bump_workflow_runs_the_script_the_gate_proves() {
    let bump = without_comments(&repo_file(".github/workflows/bump.yml"));
    let step = step_block(&bump, "raise the version");
    assert!(
        step.contains("sh .github/scripts/raise-version.sh '${{ steps.version.outputs.next }}'"),
        "bump.yml's raise step does not run raise-version.sh with the computed version, \
         so the gate over the script proves nothing about the release: {step}"
    );
    // The gate that drives the script is Unix only, which is sound while the
    // job that runs it stays on a Unix runner.
    let runs_on = bump
        .lines()
        .find_map(|l| l.trim().strip_prefix("runs-on:"))
        .map(str::trim)
        .expect("bump.yml declares a runner");
    assert_eq!(
        runs_on, "ubuntu-latest",
        "the bump moved to {runs_on:?}, so the Unix-only gate over raise-version.sh \
         no longer covers where it runs"
    );
}

/// What each document is allowed to weigh, in bytes.
///
/// The two a session reads before anything else are in the table, because
/// they sit in the same context window as the work: a rule stated three
/// times in the skill costs the pass the room it needs to reason.
const WRITTEN_LAYER_BUDGET: [(&str, usize); 6] = [
    ("SPEC.md", 390470),
    ("REVOCATIONS.md", 6560),
    ("ROADMAP.md", 95494),
    ("RULINGS.md", 95129),
    ("CLAUDE.md", 17304),
    (".claude/skills/take-next/SKILL.md", 25813),
];

/// The graviola release whose `verify_cpu_features` the shell's guard mirrors.
///
/// Bumping this means re-reading `low/x86_64/cpu.rs` and `low/aarch64/cpu.rs`
/// in the new release and bringing `update.rs::provider_runs_here` into line.
const GRAVIOLA_MIRRORED: &str = "0.4.1";

/// The mirrored CPU-feature list is pinned to the release it was read from.
///
/// The guard exists because graviola asserts rather than degrades and this
/// workspace aborts rather than unwinds, so a feature added upstream is a dead
/// monitor on hardware nobody here can test. A caret range would take that
/// silently, and no test of the guard's own behaviour can see it: the guard
/// keeps answering, about the wrong list.
#[test]
fn the_cpu_guard_still_mirrors_the_release_it_was_read_from() {
    let lock = repo_file("Cargo.lock");
    let resolved = lock
        .split("[[package]]")
        .find(|block| block.contains("name = \"graviola\""))
        .and_then(|block| {
            block
                .lines()
                .find_map(|line| line.trim().strip_prefix("version = "))
        })
        .map(|version| version.trim_matches('"').to_owned())
        .expect("Cargo.lock resolves graviola");

    assert_eq!(
        resolved, GRAVIOLA_MIRRORED,
        "graviola moved to {resolved}. Re-read `verify_cpu_features` in `low/x86_64/cpu.rs` and `low/aarch64/cpu.rs`, bring `update.rs::provider_runs_here` into line with whatever it asserts on now, then move this constant. A feature added there and missed here is an abort on a reader's machine"
    );

    // Comment markers stripped and whitespace collapsed, so the phrase is found
    // wherever the docblock happens to break its lines.
    let guard = read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src/update.rs"));
    let prose: String = guard
        .lines()
        .map(|line| line.trim().trim_start_matches('/').trim())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        prose.contains(&format!("graviola {GRAVIOLA_MIRRORED}")),
        "the guard no longer names the release its list came from, so the next reader cannot tell which source to check it against"
    );
}

/// What the documents together are allowed to weigh.
///
/// A ceiling may rise only while another falls by at least as much. The per-file
/// checks cannot see that trade; this exists to.
///
/// A ledger row is the exception it cannot express: a withdrawal recorded or an
/// issue reopened cannot be declined to fit, which is #374.
const WRITTEN_LAYER_TOTAL: usize = 630770;

/// Each document weighs no more than its budget.
#[test]
fn the_written_layer_stays_under_its_budget() {
    let root = repo_root();
    let mut over = Vec::new();

    for (name, ceiling) in WRITTEN_LAYER_BUDGET {
        let bytes = read(&root.join(name)).len();
        if bytes > ceiling {
            over.push(format!(
                " {name}: {bytes} bytes against a ceiling of {ceiling}"
            ));
        }
    }

    assert!(
        over.is_empty(),
        "the written layer grew past its budget:\n{}\n\nLower the ceiling when prose \
         comes out. Raising one to fit what was added is what this refuses, and the \
         total below is the only exception: a ceiling may rise while another falls by \
         at least as much.",
        over.join("\n")
    );

    let sum: usize = WRITTEN_LAYER_BUDGET.iter().map(|(_, c)| c).sum();
    assert!(
        sum <= WRITTEN_LAYER_TOTAL,
        "the ceilings above now sum to {sum} against a written layer of \
         {WRITTEN_LAYER_TOTAL}. One of them was raised without another falling by \
         as much, which is the per-file ceiling being edited to fit rather than a \
         ruling moving house."
    );
}
