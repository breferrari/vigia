//! What the published `.crate` carries, and what the release pipeline does.
//!
//! `SPEC.md` §9 is a contract with no code under it: it describes a tarball
//! nobody builds during `cargo test`, a workflow that only runs on a tag, and a
//! set of targets named in three separate files. Every one of those is a claim
//! this repository could break without a single gate going red, and one of them
//! already had. §9 said "three escapes" across two files for two phases while the
//! real number grew to thirteen, because the count lived in prose and ten tests
//! were added by people with no reason to edit a sentence in another file.
//!
//! So this file is §9's teeth. It reads the manifests and the workflows the way
//! `soak.rs` reads `.github/workflows/soak.yml`: through `CARGO_MANIFEST_DIR`,
//! hand-parsed, because a TOML or YAML parser is a dependency `SPEC.md` does not
//! name and the properties here are line-shaped rather than tree-shaped.
//!
//! **This file escapes the package it is about**, which is not irony but the
//! only way the check can exist: everything it compares lives above
//! `crates/vigia/`. `exclude = ["tests/**"]` covers it along with every other
//! test, which is what the first gate below asserts.
//!
//! What none of this proves: nothing here builds a tarball or runs a workflow.
//! [`the_packaged_artifact_carries_no_tests`] and
//! [`every_published_crate_ships_the_licence`] are the gates that ask cargo
//! rather than asking a file, and both go through [`package_list`], so both
//! skip together when the registry is away. A syntactically broken workflow
//! still reaches CI. `RELEASE-SMOKE.md` is where the artifact itself gets
//! checked, by a human, before the tag that makes any of it permanent.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The two shapes a test uses to read outside this package.
///
/// **Assembled with `concat!` rather than written whole, so that this file does
/// not match itself.** The needles are the only strings in the repository whose
/// presence *means* "something escapes here", so a scanner that spells them
/// literally is guaranteed to find one in its own source. That is not a
/// harmless false positive: `package.rs` genuinely does escape, through
/// [`repo_root`], so the scanner reported the right answer by the wrong
/// mechanism, and the wrong mechanism is the one that survives. Rewriting
/// [`repo_root`] to climb some other way would have silently stopped this file
/// being detected while it went on escaping, and every gate below would have
/// stayed green.
///
/// Verified rather than reasoned: before this split, `escapes(package.rs)` was
/// true with zero `#[path]` attributes in the file.
const PATH_ATTRIBUTE: &str = concat!("#[path = \"..", "/../");
const CLIMBING_LITERAL: &str = concat!("\"..", "/..");

/// How many of `vigia`'s test files read outside the package.
///
/// Stated once here and asserted against the *documents* that repeat it, rather
/// than written into an assertion and left to agree with them by hand. Three
/// files spell this number in prose, and the whole reason this test file exists
/// is that a number living only in prose drifted by a factor of four. Fixing
/// that with a number living only in a test would have been the same mistake
/// with a smaller radius.
const ESCAPING_FILES: usize = 15;

/// The English spelling of [`ESCAPING_FILES`], which is how the prose says it.
const ESCAPING_FILES_SPELLED: &str = "fifteen";

/// The repository root, two levels above this package.
///
/// Spelled `join("../..")` rather than `join("..").join("..")` so the climb
/// appears in this file's source as [`CLIMBING_LITERAL`]. That makes
/// `package.rs` detectable by the same rule as every other escaping test,
/// rather than exempt from its own gate by an accident of spelling.
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
    // Twenty-one today. The floor is set just under that rather than at some round
    // number well below it, because the only thing this guards is the scan
    // pointing at the wrong directory, and a loose floor makes that survivable:
    // `> 10` would still pass if half the suite went missing.
    assert!(
        found.len() >= 20,
        "found only {} test files, which means the scan is looking in the wrong \
         place and every assertion built on it is vacuous",
        found.len()
    );
    found
}

/// Does this test's source read anything outside the package?
///
/// Two shapes, and both are needed. A `#[path]` attribute climbing out is how
/// twelve files reach `vigia-core/tests/support/mod.rs`. A `CARGO_MANIFEST_DIR`
/// join climbing out is how `soak.rs` reads the soak workflow and how this file
/// reads `SPEC.md`.
///
/// The `CARGO_MANIFEST_DIR` half is matched on the *climbing literal* rather
/// than on the `env!` alone, because `soak.rs` uses `CARGO_MANIFEST_DIR` twice
/// and only one of the two leaves: `join("tests/soak.rs")` stays inside, and the
/// workflow path does not. A check that counted every mention would call the
/// first one an escape and be wrong in the direction that looks thorough.
fn escapes(source: &str) -> bool {
    source.contains(PATH_ATTRIBUTE)
        || (source.contains("CARGO_MANIFEST_DIR") && source.contains(CLIMBING_LITERAL))
}

/// Every test file that reads outside the package, by name, sorted.
///
/// One function rather than two predicates and a filter written at each call
/// site: two tests need exactly this list, and a fourth escape shape should
/// be teachable in one place.
///
/// **The vacuity guards live here rather than in one caller**, because every
/// caller is a `for` loop over this list and every one of them passes trivially
/// if it comes back empty. Putting them in the producer means a scanner that
/// stops scanning fails both at once instead of quietly satisfying one.
fn escaping_tests() -> Vec<String> {
    let escaping: Vec<String> = test_files()
        .into_iter()
        .filter(|(_, source)| escapes(source))
        .map(|(name, _)| name)
        .collect();

    // **Exactly fifteen, not at least.** A floor was the first spelling and it
    // reopens the very defect this file exists to close: adding a sixteenth
    // escaping test passes the floor, so `SPEC.md` §9, `crates/vigia/Cargo.toml`
    // and `RELEASE-SMOKE.md` all go on saying "fifteen" with nothing red. The
    // whole point is that a count in prose cannot notice a new test, and a
    // floor is a count that cannot notice one either.
    //
    // The cost is that adding a test which escapes now requires editing this
    // number, which is the intended cost: it is one line, and it is attached to
    // the three documents that have to change with it.
    assert_eq!(
        escaping.len(),
        ESCAPING_FILES,
        "the three documents that state this count all say {ESCAPING_FILES}. \
         Found {}: {escaping:?}. Update them together",
        escaping.len()
    );
    // This file must be among them, which is the check with the sharpest teeth:
    // `package.rs` reads `SPEC.md` and three workflows, so if the scanner stops
    // seeing it, the scanner is broken rather than the repository clean. It was
    // detected for the wrong reason once already; see `PATH_ATTRIBUTE`.
    assert!(
        escaping.iter().any(|name| name == "package.rs"),
        "the scanner no longer detects its own escape, so it has stopped \
         working: {escaping:?}"
    );

    escaping
}

/// The entries of a TOML array declared as `key = [ … ]`.
///
/// Handles the one-line and the multi-line spellings identically, because it
/// splits on commas rather than on lines. That matters: `exclude` is written on
/// one line today and `publish-jobs` could be rewrapped by anyone, and a parser
/// that read one entry per line would return nothing for the other shape and be
/// wrong silently.
fn toml_array(source: &str, key: &str) -> Vec<String> {
    // Comments first, on the same rule as `without_comments` for YAML: a `#`
    // anywhere inside a multi-line array would otherwise be split on commas
    // along with everything else and produce entries made of prose. Every array
    // this reads is a list of bare strings, so there is no `#`-inside-a-value
    // case to protect.
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
///
/// **Written as a small parser with its own test because two attempts at doing
/// it with a line filter were both wrong, in opposite directions.** The first
/// read only `run: <command>`, so rewriting a step as a `run: |` block, which
/// is what anyone does the moment it needs two lines, reported that the file
/// contained no publish command at all. The second dropped the `run:`
/// requirement to fix that, and thereby matched *any* line, so a step called
/// `- name: publish (was cargo publish, split for retries)` satisfied the gate
/// while the command beneath it did something else. That second failure is the
/// one the gate was written to catch in the first place.
///
/// The shapes it reads:
///
/// - `run: cargo publish …` on one line, with or without the list dash.
/// - A block scalar with an indented body, joined into one string. `|` and `>`,
///   and either carrying a chomping indicator or an indentation digit, since
///   `|-` is ordinary YAML and reading only the bare form turned a legitimate
///   rewrite into a red claiming the file had no publish command at all.
/// - Either of those with `\` continuations, joined, because a flag on a
///   continued line is invisible to anything working line by line and
///   `--dry-run` is exactly the flag someone would put there.
///
/// Nothing else on a step is a command, which is the property both earlier
/// versions lost: a `name:`, an `if:` or a comment may say anything at all.
fn run_commands(yaml: &str) -> Vec<String> {
    let lines: Vec<&str> = yaml.lines().collect();
    let mut commands = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim_start();
        // `- run:` as well as `run:`, since the first step of a list carries the
        // dash on the same line as its first key.
        // The dash form carries the key on the same line as the list marker, so
        // the indent that bounds a block body is the **key's**, not the dash's.
        // Measuring from the dash makes the body look two columns wider than it
        // is, and the scan then swallows the step's sibling `env:` and `if:`
        // keys into the command.
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

        // **Both arms absorb the continuation lines, and only the block one did
        // at first.** The inline arm took its single line and moved on, which
        // was fail-open on precisely the flag this scan exists to reject: the
        // real publish step *is* the inline form, so appending
        // `\` and `--dry-run` on the next line gave GitHub one command carrying
        // `--dry-run` and gave this scan one command without it. Every
        // assertion passed while the release published nothing.
        //
        // A more-indented line after a key belongs to that key's value in YAML
        // whichever form it took, so the collection rule is the same for both
        // and only the first line differs.
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
///
/// **The distinction decides whether the gate below skips or fires**, and the
/// first attempt at it was wrong in the dangerous direction *and* the annoying
/// one at once. It listed five substrings written from memory, none of which
/// cargo 1.94 actually emits for the common offline shape, so an offline
/// developer got a hard red from a gate that had nothing to say.
///
/// The strings here are **captured from real runs** rather than recalled, which
/// is the whole reason [`the_unreachable_registry_is_told_apart_from_a_broken_manifest`]
/// exists beside this: a list of magic strings nobody has ever matched against
/// real output is prose wearing a `const`. Two shapes were observed on
/// 2026-08-08 with cargo 1.94.0:
///
/// - Cold `--offline`: `no matching package named 'notify' found`, followed by
///   `you're using offline mode (--offline)`. Note that the first line alone is
///   indistinguishable from a genuinely missing dependency, which is why the
///   marker is the *offline* sentence rather than the failure.
/// - A broken manifest: `readme 'READMEE.md' does not appear to exist`, which
///   matches nothing here and so correctly fires the gate.
///
/// Case-insensitive because cargo capitalises some of these mid-sentence
/// (`Could not connect to server`) and not others.
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
    // **Only the lines cargo failed on.** A network blip that cargo *recovers*
    // from prints `warning: spurious network error` and then carries on, so
    // matching the whole stderr would let a recovered blip vouch for a run that
    // went on to fail for an unrelated reason, skipping the gate on a broken
    // manifest. The failure is what is being classified, not the transcript.
    let lowered = stderr.to_lowercase();
    lowered
        .lines()
        .filter(|line| !line.trim_start().starts_with("warning:"))
        .any(|line| MARKERS.iter().any(|marker| line.contains(marker)))
}

/// Assert that `preflight` carries a create-then-delete write probe against
/// `repo`, anchored on the shell variable naming the ref it creates.
///
/// **Bounded to one probe, because there are two now and unbounded assertions
/// stopped telling them apart.** The tap's gate asserted `-X POST`, `/git/refs`,
/// `-X DELETE` and `201` over the whole step. When a second probe landed in that
/// same step, every one of those strings had two suppliers: deleting the tap
/// probe entirely left the gate green, and the gate exists because v0.1.0
/// actually failed there. That is the fourth time in this file a mention has
/// stood in for the thing, and the first time the mention was a *different real
/// mechanism* rather than a comment.
///
/// So each assertion is confined to its own probe's span: from the line naming
/// the ref, through the create, to the request that undoes it. A probe deleted
/// now takes its span with it and its own gate fails.
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
    //
    // **The comparison *and* the stop.** Asserting only `!= "201"` leaves the
    // probe decorative: replace the failure body with an `echo` and the string
    // survives, the job carries on, and the irreversible half proceeds against
    // a token that has just been shown not to work. Demonstrated by running it,
    // not reasoned about.
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
///
/// **A step is more than the commands it runs, and [`run_commands`] only ever
/// sees the commands.** An `if:` or a `continue-on-error:` beside them decides
/// whether any of it executes, and both are invisible to every gate in this file
/// that reads a `run:` body. Adding `if: ${{ inputs.rehearse }}` to the token
/// pre-flight leaves the entire suite green while switching off the guard that
/// exists because a release actually failed.
fn step_block<'a>(bump: &'a str, name: &str) -> &'a str {
    let header = format!("- name: {name}");
    let at = bump
        .find(&header)
        .unwrap_or_else(|| panic!("bump.yml has no step named `{name}`"));
    let rest = &bump[at + header.len()..];
    rest.find("\n      - ").map_or(rest, |end| &rest[..end])
}

/// The mapping keys sitting at exactly `indent` columns, unquoted.
///
/// **Not a substring search for `if:`, because four spellings evade one.** A
/// quoted `"if":`, an `if :` with a space before the colon, the key at a
/// different indent, and the same key one level up on the *job* are all valid
/// YAML and all mean the step does not run. Each of those passed a `contains`
/// check that had been mutation-tested against the obvious spelling only, which
/// is what a single mutation buys: confidence in the one case you thought of.
///
/// Lines inside a `run: |` body are indented deeper than any key this asks
/// about, so shell `if [ ... ]` is never mistaken for the YAML key.
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
///
/// **The rehearsal's whole promise is that `main` is left alone**, and until
/// this existed that promise had no gate: deleting the condition from `commit
/// the bump` left the suite green while every rehearsal pushed a version to the
/// default branch, with the step above it still printing "main untouched".
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
    // **The whole condition, not a substring of it.** `!inputs.rehearse ||
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
///
/// **A step-level check cannot see the level above it.** Moving the pre-flight
/// into a second job that nothing `needs:` leaves every step-level assertion
/// true while the commit runs whether or not the tokens were ever checked, and
/// an `if:` on the job skips every step in it at once. Both parsed as valid
/// YAML and both passed before this existed.
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
///
/// Both token gates need this and both spelled it out: a check that reports a
/// problem after the version has already moved reports it too late.
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
///
/// This repository comments heavily by house style, and several of those
/// comments quote the very command or triple a gate below looks for. Without
/// this, commenting out a `run:` line and leaving its explanation above it keeps
/// the gate green, which is the failure a comment-blind `contains` is guaranteed
/// to have eventually.
///
/// Used for both YAML and TOML, which share `#` and, in every file read here,
/// share the property that no value legitimately contains one. That is worth
/// stating because it is the limit: this would truncate a genuine `#` inside a
/// quoted string, so it is a helper for these files rather than a comment
/// stripper in general.
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
///
/// Only the shapes this repository actually uses are understood: a literal path
/// and a trailing `**`. Anything else is rejected loudly rather than silently
/// treated as no-match, because a pattern this function does not understand is
/// exactly the case where a silent "no" reads as a finding and is a bug here.
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
///
/// **Derived rather than written out, on the same rule as everything else in
/// this file.** A list restated here is a second place to edit, and forgetting
/// is silent in the direction that matters: `cargo publish --workspace` ships a
/// third crate whether or not any gate below has heard of it.
/// [`the_internal_dependency_tracks_the_workspace_version`] is what makes
/// adding one a decision rather than a discovery; this is what makes the gates
/// follow it.
///
/// The name is read from the member's own `[package]` block rather than taken
/// from the last path segment. They agree today, and a directory name is not a
/// crates.io name: `cargo package -p <name>` takes the latter, so reading the
/// manifest is the mechanism and the path is a guess that happens to be right.
fn workspace_members() -> Vec<(String, String)> {
    toml_array(&repo_file("Cargo.toml"), "members")
        .into_iter()
        .map(|dir| {
            // **Comments stripped first**, which is the rule every other parser
            // in this file follows and the one a line-prefix scan needs most: a
            // `# name = "the-old-name"` sitting between `[package]` and the real
            // field is exactly what someone leaves behind while renaming a
            // crate, and `find_map` would take it. `toml_array` strips for the
            // same reason, and there is no `#`-inside-a-value case here because
            // a crates.io name cannot contain one.
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
///
/// The licence gates walk this list rather than [`workspace_members`], because
/// what they guarantee is a property of the **published** artifact: `xtask`
/// (`publish = false`, the grammar-dump builder `SPEC.md` §6 names) never
/// becomes a `.crate`, so there is no tarball for a LICENSE to be missing
/// from. The member-list pin in
/// [`the_release_pipeline_publishes_to_the_registry`] is what stops an
/// unpublished member from being added silently.
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
///
/// **Trimmed equality rather than `contains`**, which is this file's standing
/// rule and the one it has been caught by four times: a substring test for
/// `LICENSE` would also be satisfied by a path ending in it, so the mention
/// would stand in for the thing. The list prints package-relative paths one per
/// line, so an exact line is the mechanism and nothing else is.
fn listed_has(listed: &str, file: &str) -> bool {
    listed.lines().any(|line| line.trim() == file)
}

/// `cargo package --list` for one member, or `None` if the registry is away.
///
/// **The skip is a documented outcome rather than a failure**, and the
/// distinction is [`registry_unreachable`]'s: an index that cannot be reached
/// means this gate proved nothing on this run, while anything else cargo
/// refuses is the gate firing. `gate` names the caller in the annotation, so a
/// CI log says which claim went unchecked rather than that one did.
///
/// **`gate` is a literal that has to track the caller's own name, and nothing
/// enforces that**, so a renamed test leaves a CI annotation pointing at a test
/// that no longer exists. Recorded rather than fixed: Rust has no stable way to
/// read the enclosing function's name, and the alternatives are a
/// `stringify!`-based macro or a `type_name` trick that are both more machinery
/// than two call sites are worth. The failure is a misleading log line on a run
/// that already proved nothing, which is the cheapest place in this file for a
/// drift to land.
fn package_list(package: &str, gate: &str) -> Option<String> {
    let output = Command::new(env!("CARGO"))
        .args(["package", "--list", "--allow-dirty", "-p", package])
        .current_dir(repo_root())
        // **Colour off, or the classifier below cannot see a `warning:`.**
        // `ci.yml` sets `CARGO_TERM_COLOR: always` for the whole workflow, so in
        // CI cargo writes `\e[1m\e[93mwarning\e[0m:` and a prefix test for
        // `warning:` is false on every line. That would put the one filter that
        // stops a recovered network blip excusing a broken manifest out of
        // action precisely where it matters, and nowhere else.
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
///
/// The invariant `SPEC.md` §9 states and could not enforce. A test that escapes
/// cannot compile in an unpacked or vendored copy, because the thing it reaches
/// for is not in the tarball, so shipping one means `cargo test` fails for a
/// reader who did nothing wrong.
///
/// The resolution is directory-wide (`exclude = ["tests/**"]`) rather than
/// per-file, and that is deliberate: fifteen of the twenty-one test files escape
/// already, a per-file list would need editing every time a test is added, and
/// the failure mode of forgetting is silent. This gate holds either shape,
/// because it asks whether each escaping file is *covered*, not how.
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
///
/// **This is the gate against the exact defect that produced it.** §9's own
/// closing sentence says the escapes are "counted rather than described because
/// a count is what a later reader can check", and then nothing checked it: the
/// bullet said three across two files while the truth grew to thirteen across
/// twelve. Prose cannot notice a new test.
///
/// One direction only. Every escaping file must be named in the bullet; the
/// bullet is free to mention others, because it also discusses `vigia-core`'s
/// files in order to explain why that package is *not* excluded.
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

    // **And every document that states the count states the right one.**
    // Asserting the number in a test and leaving the three files to agree with
    // it by hand is the same defect with a smaller radius: bumping the constant
    // for a sixteenth escaping test would otherwise leave all three saying
    // "fifteen", green.
    // Each document's own sentence, not the bare numeral, and the reason survives
    // every bump rather than being about one number: a bare `contains("fifteen")`
    // is satisfied by "fifteen**th**" the moment the manifest writes about a
    // sixteenth escape, and the numeral for the previous count was already sitting
    // in SPEC.md about column widths, so two of the three documents would pass
    // untouched while still saying the old number. Found at thirteen, paid for at
    // fourteen, and it is why these are phrases: a count assertion an unrelated
    // numeral satisfies is the same failure as a count in prose.
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
///
/// Every case here is one an audit round actually got past a previous version
/// of this scan, so the list is a record of how it was wrong rather than a
/// survey of YAML. Two of them are the same defect from opposite sides: a
/// `name:` that quotes a command is not a command, and a command split over two
/// physical lines is still one.
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

    // **A continuation after the *inline* form.** This is the shape the real
    // publish step has, and the arm that handled it took one line and stopped,
    // so `--dry-run` on the next line was invisible to the assertion that
    // exists to reject it while GitHub ran it. Fail-open, on the one flag that
    // makes the whole job a no-op.
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
///
/// A gate that skips has to be right about *when*, and both directions cost
/// something real: skipping on a broken manifest hides a release defect, and
/// firing on an offline machine breaks development for a reason the developer
/// cannot act on. The condition was written from memory once and was wrong both
/// ways, so the fixtures below are pasted from actual `cargo package --list`
/// runs on 2026-08-08 with cargo 1.94.0 rather than composed.
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

    // Every marker is exercised, because a marker nobody has ever matched is
    // the thing this whole function was rewritten to stop being. Four of the
    // seven were unreached by the cases above, which is the same "prose wearing
    // a const" the docblock complains about, one level in.
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
///
/// **The one duplication in this manifest that cargo cannot remove.** A path
/// dependency that will be published needs both halves: `path` is what a
/// checkout builds against, and `version` is what `cargo publish` rewrites the
/// dependency to, because crates.io has no paths. Cargo has no
/// `version.workspace = true` inside a dependency spec, so the number is written
/// twice and only a gate can hold the two together.
///
/// The failure it prevents is silent, permanent, and passes every other check
/// here: bump the workspace to 0.2.0 while the pin still reads 0.1.0, and
/// `cargo publish --workspace` ships `vigia` 0.2.0 depending on `vigia-core`
/// **0.1.0** — a real published crate that resolves, builds and installs. The
/// binary a user gets is the new shell over the old engine, and nothing in the
/// repository is red.
#[test]
fn the_internal_dependency_tracks_the_workspace_version() {
    let root = repo_file("Cargo.toml");
    let clean = without_comments(&root);

    // **Scoped to the `[workspace.package]` block, not the first `version = `
    // in the file.** The unscoped form was demonstrated green against a stale
    // pin: put any table declaring a `version` above `[workspace.package]`, and
    // this read *that* one, compared it against a `vigia-core` pin of the same
    // number, and reported agreement while the workspace sat at a different
    // version. A vacuity guard on the shape of the string cannot see that,
    // because both strings are perfectly well-formed versions.
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

    // And the shell must still *use* the workspace entry. Everything above
    // checks the entry is correct; none of it notices if `crates/vigia` stops
    // reading it and inlines its own `version` again, which is precisely the
    // shape this whole gate was written against, one file over.
    let shell = read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"));
    assert!(
        without_comments(&shell).contains("vigia-core.workspace = true"),
        "crates/vigia/Cargo.toml no longer takes vigia-core from \
         [workspace.dependencies], so the pin checked above is not the one \
         being published"
    );
}

/// Nothing in `exclude` has quietly stopped matching anything.
///
/// A stale pattern is worse than a missing one: it reads as protection, it
/// survives review because deleting an `exclude` entry always looks risky, and
/// it excludes nothing at all. The failure it hides is the same one the gate
/// above exists for, arrived at from the other side.
#[test]
fn nothing_excluded_has_since_stopped_existing() {
    let names: Vec<String> = test_files().into_iter().map(|(name, _)| name).collect();

    // Both arms of `covers` are exercised here rather than left to whatever
    // `exclude` happens to hold. The literal arm is unreachable from the current
    // manifest, and an unreachable branch in a gate's own helper is the thing
    // that breaks the first time someone narrows the pattern.
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

    // And the third arm, the one that refuses to guess. `covers` panics rather
    // than answering "no" for a pattern shape it does not understand, because a
    // silent no here reads as a finding, and that arm was unexercised while the
    // comment above claimed both arms were covered.
    let unknown = std::panic::catch_unwind(|| covers("tests/*.rs", "soak.rs"));
    assert!(
        unknown.is_err(),
        "a mid-string glob must be refused loudly, not answered `no`, or a \
         pattern this gate cannot evaluate reads as an escape it has caught"
    );

    // Only patterns aimed at `tests/` are checked against test names. This gate
    // is about the exclusion that keeps escaping tests out of the tarball; an
    // `assets/**` added later for an unrelated reason is not stale merely
    // because no test matches it, and asserting otherwise would make a correct
    // edit go red.
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
///
/// Every absolute budget in this repository (I7, I9, I3) is taken in `release`.
/// `dist` builds with `[profile.dist]`, so the moment those two diverge, every
/// one of those numbers becomes a claim about a binary no user receives, and
/// nothing anywhere would say so: a tuned `dist` profile builds, ships and
/// installs exactly like an untuned one.
///
/// The assertion is that the block inherits `release` **and adds nothing**,
/// rather than that it adds nothing *harmful*. Deciding which keys are harmless
/// needs a model of the optimiser this gate has no business holding, and "adds
/// nothing" is a property a reader can confirm in one glance.
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
    // `[profile.dist.package."gix"]` would sit *outside* what it read and tune
    // the release build without this assertion ever seeing it. Cargo supports
    // per-package profile overrides, so that is a real edit somebody could make
    // in good faith, and the block scan cannot be widened to catch it without
    // becoming a TOML parser.
    assert!(
        !root.contains("\n[profile.dist."),
        "a [profile.dist.*] sub-table overrides the shipped profile where the \
         block scan above cannot see it; the budgets would stop describing the \
         binary and this gate would stay green"
    );
}

/// The release pipeline actually sends both crates to the registry.
///
/// `dist` has no built-in crates.io publisher, so this is a custom reusable
/// workflow, which means it is wired by a string in one file and implemented in
/// another. A typo in either produces a release that builds binaries, publishes
/// a Homebrew formula, announces itself, and never claims the crate name, and
/// the first symptom is a user's `cargo install vigia` failing weeks later.
///
/// `--workspace` is asserted by name rather than left to whatever `cargo publish`
/// invocation happens to be there. Two `-p` calls in sequence are the obvious
/// alternative and they are wrong: `vigia` declares `vigia-core` by path *and*
/// version, so the registry refuses the second until the first is indexed, and
/// the sleep that papers over it has no correct length.
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

    // Comment-stripped, because this repository comments heavily and the
    // paragraph above the `run:` line in that workflow quotes the very command
    // asserted here. Against the raw text, commenting out the publish and
    // leaving its explanation in place keeps this gate green.
    let job = without_comments(&repo_file(WORKFLOW));
    assert!(
        job.contains("workflow_call"),
        "{WORKFLOW} must be a reusable workflow; dist calls it with `uses:`"
    );

    // **Only a `run:` body counts as publishing.** Stripping comments was not
    // enough: a step called
    // `- name: publish (was cargo publish --workspace, split for retries)`
    // satisfies a whole-file `contains` while the `run:` beneath it does
    // something else entirely, and that is a plausible edit rather than a
    // contrived one, because it is what somebody writes while debugging a
    // failed release. So the search is scoped to the command lines.
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

    // **The one flag that makes this whole job a no-op.** Every assertion above
    // passes with `--dry-run` appended, and so does the release: binaries build,
    // the tap is written, the announcement goes out, and the name is never
    // claimed. It is also the likeliest edit anybody makes, because it is what
    // you add to test the workflow and the easiest thing to forget to remove.
    assert!(
        !publish.contains("--dry-run"),
        "{publish:?} carries --dry-run, so the release would do everything \
         except the publish and report success"
    );

    // `--workspace` publishes every member that does not opt out, so what the
    // workspace *holds* is part of what the release ships. A crate added for
    // any reason, a fixture generator, a proc macro, an experiment, would be
    // published to crates.io permanently on the next tag, and nothing else
    // here would mention it. The member list is therefore pinned whole: two
    // published crates is a deliberate number (`SPEC.md` §6's engine/shell
    // split), `xtask` is deliberately unpublished (`publish = false`, §6's
    // grammar-dump builder), and any new name is a decision that should be
    // made here rather than discovered at the tag.
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

    // dist generates release.yml from the config above, so this asserts the
    // generated file is current rather than that the config is right. A stale
    // release.yml is the failure mode of editing the config and not
    // regenerating, and it is a real one **for the publish wiring specifically**:
    // measured 2026-08-08, dropping `./publish-crates-io` from the config and
    // not regenerating leaves `dist generate --check` red and both assertions
    // below red too, while the *target* list bakes into release.yml not at all
    // (zero triples appear in it; the build matrix is computed at release time
    // from `dist plan`). So this checks the half that can actually go stale, and
    // `ci.yml`'s `dist generate --check` covers the whole file.
    let release = without_comments(&repo_file(".github/workflows/release.yml"));
    assert!(
        release.contains("uses: ./.github/workflows/publish-crates-io.yml"),
        "release.yml does not call the registry job; run `dist generate`"
    );
    assert!(
        release.contains("- custom-publish-crates-io"),
        "release.yml must schedule the registry job at all; run `dist generate`"
    );

    // **The ordering claim this used to make was false, so what is asserted is
    // the mechanism rather than the conclusion.** The old wording said
    // `announce` waiting on the publish job stopped a release half-shipping. It
    // does not: `host` runs `gh release create` with no `--draft`, so the
    // binaries are public before the registry job starts.
    //
    // Asserted as two facts a reader can check, not as a byte-offset
    // comparison. Textual order in a YAML mapping is not semantic, so an
    // offset comparison both false-reds (dist reorders its emitted jobs and
    // nothing has changed) and false-greens on the edit that actually matters:
    // adding `--draft` to that line moves no offsets at all, and dist's own
    // stated direction is draft-then-undraft, so a future upgrade lands
    // exactly that.
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
    // The flag can also arrive through a variable, which is exactly how the
    // prerelease flag on that same line is passed, so reading the literal alone
    // is defeated by dist's own idiom. Any `DRAFT` in the generated file means
    // the assumption above needs re-reading rather than trusting.
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
///
/// **There used to be a test here asserting the two lists agreed, and deleting
/// it is the fix rather than a retreat from it.** `CLAUDE.md` calls a C build
/// dependency in the graph a spec change rather than an implementation detail,
/// and `ci.yml`'s `pure-rust` job is that rule with teeth; it held a hand-typed
/// target list while `[workspace.metadata.dist]` held another. Shipping a fourth
/// target was the moment they came apart, and the gate caught it:
/// `x86_64-apple-darwin` was in the release and the purity job had never heard
/// of it.
///
/// A gate over a duplication fires *after* somebody types the fifth target
/// wrong. Deriving the list makes the drift impossible instead, so what this
/// asserts now is only that the derivation is still there: `cargo metadata`
/// exposes `[workspace.metadata.dist]` verbatim, and the job reads it.
#[test]
fn the_purity_gate_derives_its_targets_from_the_release_config() {
    // Comment-stripped before slicing, because the region is heavily commented
    // and those comments discuss targets by name. The non-vacuity check below
    // asks whether a triple is spelled literally in the job, and prose about a
    // triple is not the job spelling one.
    let ci = without_comments(&repo_file(".github/workflows/ci.yml"));
    // Bounded at both ends. Taking everything *after* the marker was the first
    // spelling and it reached into the `musl` job below, which names its target
    // literally and for good reason, so the non-vacuity check below fired on a
    // different job's line. `exit $status` is this script's last line.
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
///
/// **Three files have to agree and only one of them fails loudly on its own.**
/// `bump.yml` dispatches `release.yml`; `release.yml` accepts a dispatch only
/// because `[workspace.metadata.dist] dispatch-releases = true` generated it
/// that way; and that same setting is what removed the tag-push trigger. Turn
/// the setting off and regenerate, and `bump.yml` still exists, still has its
/// dropdown, still bumps the version and still pushes the commit, and then
/// dispatches a workflow that has no `workflow_dispatch` to receive it. The
/// release never runs. Nothing is red, the version has already moved, and the
/// tell is a release that simply did not happen.
///
/// That is worth a gate rather than a comment because the failure is silent in
/// the direction that matters and because the whole mechanism exists to avoid a
/// second long-lived token: a tag pushed from a workflow holding `GITHUB_TOKEN`
/// triggers nothing, and `workflow_dispatch` is the exception being relied on.
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

    // **The word immediately after `gh workflow run`, not a mention anywhere in
    // the step.** A `run: |` body is one string here, and that body ends with an
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

    // **All three tokens are checked before the version moves, and the two that
    // must write are checked for `push`.** This is v0.1.0's actual failure as a
    // gate. `HOMEBREW_TAP_TOKEN` could read the tap and not write to it; the
    // formula job checked the tap out successfully and then failed `git push`
    // with a 403, by which point the GitHub release and the crates.io publish
    // had both already happened.
    //
    // Asserting `permissions.push` rather than merely that a check exists is
    // the point. A check that only proves the token *works* passes against a
    // read-only one, because reading a public repository needs no grant at all,
    // so a token scoped to entirely the wrong repository reads this one fine.
    let preflight = commands
        .iter()
        .find(|command| command.contains(r#"-z "${CARGO_REGISTRY_TOKEN"#))
        .expect(
            "bump.yml must guard on the registry token being empty before it              bumps anything. Finding the *name* is not enough: it appears in              this step's own error message and in its `env:` block, so a              search for it passes against a check that has been disabled",
        );
    // Each assertion names a form only the *check* has, never one an error
    // message or an `env:` entry also carries. Both of these were written the
    // loose way first and both survived mutation: deleting the tap presence
    // check left `HOMEBREW_TAP_TOKEN` in the curl and the env block, and
    // replacing the jq path left `permissions.push` in the failure message.
    // That is the fourth time in this file a mention has stood in for the thing.
    assert!(
        preflight.contains(r#"-z "${HOMEBREW_TAP_TOKEN"#),
        "the pre-flight does not check the tap token is set, and that is the          one that failed a release: {preflight}"
    );
    // **An actual write, not a report about one.** The first version read
    // `.permissions.push` from the API, and that is a proxy which does not
    // predict the thing: it answered `true` for the very token whose `git push`
    // had been denied 403, because for a fine-grained token that field reflects
    // the user's role on the repository rather than the token's grants. So the
    // check creates a ref in the tap and deletes it, and this asserts that
    // shape rather than any wording.
    //
    // **Bounded to the tap's own probe, and it was not always.** These three
    // assertions ran over the whole step until a second probe landed in it, at
    // which point every string they look for had two suppliers and deleting the
    // tap probe outright left them green. See [`assert_write_probe`].
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
///
/// **Run 31435812487 written down as a gate.** The button's first real run built
/// everything, moved both version strings, and was rejected at the push with
/// *"7 of 7 required status checks are expected"*. `main` is protected; a commit
/// pushed with `GITHUB_TOKEN` triggers no workflow, so those checks can never
/// arrive on it; and the bot holds write rather than admin, so it cannot bypass
/// them. Retrying reaches the same answer forever.
///
/// **This is the one step in that workflow no rehearsal has ever reached**,
/// because a rehearsal's whole promise is to leave `main` alone. Three green
/// rehearsals and a token guard rewritten twice all ran over a step none of them
/// performed, which is why the gate is here rather than left to the next
/// release to discover.
///
/// Each assertion names a form only the *mechanism* has. `RELEASE_TOKEN` appears
/// in the step's `env:` block, in two error messages and in a comment, so
/// finding the name proves nothing; this file has recorded four separate
/// occasions where a mention stood in for the thing.
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

    // **A real write to *this* repository, told apart from the tap's probe by
    // the repository it names.** Both probes are the same shape on purpose, so
    // the assertions have to say which one they are looking at, in both
    // directions: see [`assert_write_probe`] for the direction that was missed.
    assert_write_probe(preflight, "mine", "${GITHUB_REPOSITORY}");

    // **The other half of the bypass, because the write probe does not imply
    // it.** A token with `Contents: Read and write` still cannot move a
    // protected branch unless the account behind it is an admin. The probe
    // above covers the grant; this covers the standing.
    //
    // Span-bound with its comparison and its stop, for the reason
    // [`assert_write_probe`] is: `contains(".permissions.admin")` alone was
    // satisfied by deleting the whole check and leaving an `echo` that named
    // it, which was demonstrated by running it rather than argued.
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

    // **No step anywhere forgives its own failure.** Named steps are checked
    // above, but `continue-on-error` is worse than an `if:` wherever it lands:
    // on `commit the bump` it lets a *failed push* dispatch the release anyway,
    // and it sets that step's outcome to `failure`, which also silences the
    // recovery notice that exists for exactly this case.
    assert!(
        !keys_at_indent(&bump, 8).contains(&"continue-on-error".to_owned()),
        "a step in bump.yml carries `continue-on-error:`, so the release can \
         proceed past a step that failed"
    );

    // **The commit happens before the dispatch.** Reordering them dispatches a
    // release for a version the default branch does not carry yet, which
    // publishes the old code under the new number.
    assert_precedes(
        &bump,
        "git commit",
        "gh workflow run",
        "bump.yml dispatches the release before it commits the version, so the \
         release would build whatever the default branch carried beforehand",
    );

    // **A rehearsal dispatches `dry-run` and a real release does not, and the
    // polarity is the assertion.** Flipping this one comparison makes every
    // rehearsal publish to crates.io for real, which is the irreversible half
    // of the release and the one thing the rehearsal path exists to avoid.
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

    // **The checkout persists no credentials, and this is the assertion the
    // push assertion above rests on.** With the default, the workflow's own
    // token sits in `.git/config` as an `Authorization` header and git prefers
    // it over the userinfo in the push URL, so the push goes out as the bot and
    // is rejected exactly as run 31435812487 was, with every other check here
    // green. Naming the token in the URL is necessary and not sufficient.
    assert!(
        bump.contains("persist-credentials: false"),
        "the checkout persists the workflow's token into .git/config, where git \
         prefers it over the token in the push URL: the push would authenticate \
         as the bot and be rejected, with this whole pre-flight passing"
    );

    // **The remote the push names carries the token.** This is the assertion the
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

    // **And it pushes the bump at the default branch.** The remote being right
    // says nothing about the refspec: `HEAD:refs/heads/scratch` authenticates
    // perfectly, lands the version somewhere nothing tags, and leaves the
    // release dispatching against a branch that never moved. Found by mutation,
    // after the remote assertion had already been mutation-tested and looked
    // like enough.
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
///
/// Every gate above reads a file and reasons about what cargo *will* do. This one
/// asks cargo, which is the only way to catch an `exclude` that is spelled
/// correctly and means something other than what it looks like.
///
/// It shells out, so it can be genuinely unavailable: `cargo package` touches
/// the registry index, and a machine with no network answers a question nobody
/// asked.
///
/// **The skip is narrow, and the first version of it was not.** That one
/// returned on *any* non-zero exit, which meant the single most valuable failure
/// it could see was the one it swallowed: a typo in `readme` makes
/// `cargo package --list` fail, so a broken manifest turned this gate green.
/// Verified, one character (`readme = "READMEE.md"`) was enough. Now only an
/// index or network failure skips, and everything else is the finding.
///
/// **And the skip is written to stderr with a CI annotation, not `println!`.**
/// libtest captures stdout for a *passing* test and discards it without
/// `--nocapture`, so the old "skipping is printed" was false exactly where it
/// mattered: in CI, a skip and a pass looked identical, which is the shape
/// `soak.rs`'s own rule exists to prevent.
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

    // The other direction, so the assertion above cannot pass because the list
    // came back empty. `README.md` is in the package only because `readme` points
    // outside this directory and cargo copies it in, which is itself worth
    // holding: a registry page with no readme reads as abandoned.
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
///
/// `license = "MIT"` is metadata: crates.io renders a badge from it and a
/// scanner reads it, and neither puts the twenty-one lines of the licence in
/// front of a reader who has the tarball. Cargo picks a licence up from the
/// **package** directory only, and this repository's is one level above both
/// packages, so for the whole of 0.1.0 through 0.17.0 the `.crate` shipped
/// none. `dist` puts `LICENSE` in all four binary archives, so the tarball was
/// the one channel that did not carry it.
///
/// **What closes it is a copy in each package directory, and the alternative is
/// worth recording because it is the one everybody reaches for first.** An
/// inherited `license-file = "LICENSE"` resolves against the workspace root and
/// works exactly like `readme`: measured 2026-08-18, it puts `LICENSE` in both
/// tarballs and rewrites the path package-relative. It also makes `cargo
/// publish` print `warning: only one of license or license-file is necessary`,
/// from the verify step rather than from `--list`, which is why it looks clean
/// until the one workflow nobody can rehearse runs it. The root manifest
/// already ruled that trade in the other direction for `homepage`, and
/// dropping `license` to silence it would give up the SPDX expression that is
/// the field's whole point. Eight of eight dependencies here, `gix` and
/// `notify` among them and both workspaces with this same layout, ship the file
/// from inside the crate directory.
///
/// **A symlink is the third idea and the worst of the three, which is worth
/// writing down because it is the one a Unix reader reaches for first.** It
/// fails twice over, independently. Git on Windows needs
/// `SeCreateSymbolicLinkPrivilege` to materialise one at all, and without it a
/// checkout writes an ordinary file whose *contents* are the string
/// `../../LICENSE`; `.gitattributes` does not save that, since `eol=lf` governs
/// line endings rather than symlink materialisation. And even where the link is
/// real, cargo does not dereference one pointing outside the package directory:
/// [cargo#5664](https://github.com/rust-lang/cargo/issues/5664) is open on
/// exactly this case, filed against `serde`, whose packaged `LICENSE-APACHE`
/// carried the literal target path instead of the licence text. Both failures
/// are silent, and both land on the tier-1 platform this project is developed
/// on. A `build.rs` cannot help either, since `cargo package` fixes the
/// tarball's file list before any build script runs.
///
/// **And this gate lives in `vigia`'s tests rather than each crate's own**,
/// which is not tidiness. `crates/vigia/Cargo.toml` excludes `tests/**`, so a
/// test here that reads outside the package ships to nobody;
/// `crates/vigia-core`'s manifest says its tests are *"deliberately not
/// excluded"* because none of them escape. A licence-drift check placed there
/// would read `../../LICENSE` and become the first escape in the one package
/// that publishes its own test suite, which is the exact defect `SPEC.md` §9
/// and this file exist to prevent. Reaching `vigia-core` from here costs
/// nothing: this gate shells `cargo package -p vigia-core` and reads its files
/// by path, and touches nothing under its `tests/`.
///
/// This gate asks cargo. [`the_licence_each_crate_ships_is_the_repository_licence`]
/// asks whether what it ships is the right text, and neither subsumes the
/// other: a copy that drifts passes here, and a mechanism that stops working
/// passes there.
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

        // **And the grammar dump, for the engine.** `vigia-core` reaches it
        // with `include_bytes!`, so a `.crate` without it does not merely lack
        // an asset: it does not compile, and the failure lands inside `cargo
        // publish` at release time rather than in any test here. It is one
        // `exclude` line away from happening, which is the same distance the
        // licence was.
        if package == "vigia-core" {
            assert!(
                listed_has(&listed, "assets/syntaxes.bin"),
                "the .crate for {package} carries no assets/syntaxes.bin, and \
                 `Highlighter::new` embeds it with `include_bytes!`, so the \
                 published crate would not build at all:\n{listed}"
            );
        }
    }
}

/// The licence each crate ships is this repository's, byte for byte.
///
/// The cost of the copy that [`every_published_crate_ships_the_licence`]
/// records: two files that can drift from the one at the root, and from each
/// other. Drift here is worse than the absence it replaced, because a crate
/// that ships *a* licence looks settled while carrying terms nobody chose.
///
/// Offline and unconditional, unlike its pair, which is deliberate: the gate
/// that can be skipped is the one about a mechanism cargo owns, and the gate
/// about what this repository is licensed under always runs.
#[test]
fn the_licence_each_crate_ships_is_the_repository_licence() {
    let root = repo_file("LICENSE");

    // Non-vacuity, on the same rule as the package lists above: an empty or
    // truncated root file would make every comparison below pass by matching
    // nothing against nothing, and three empty files are byte-identical.
    //
    // **Fifteen lines because the licence is twenty-one**, which is the number
    // the docblock above quotes, so the floor is set just under the real value
    // rather than at a round one. A loose floor is the point: MIT's text is
    // fixed, but a year or a name change moves the byte count and must not turn
    // this into a failure about nothing.
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
