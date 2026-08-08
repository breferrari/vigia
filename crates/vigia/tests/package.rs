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
//! [`the_packaged_artifact_carries_no_tests`] is the one gate that asks cargo
//! rather than asking a file, and a syntactically broken workflow still reaches
//! CI. `RELEASE-SMOKE.md` is where the artifact itself gets checked, by a human,
//! before the tag that makes any of it permanent.

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
    // Nineteen today. The floor is set just under that rather than at some round
    // number well below it, because the only thing this guards is the scan
    // pointing at the wrong directory, and a loose floor makes that survivable:
    // `> 10` would still pass if half the suite went missing.
    assert!(
        found.len() >= 18,
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
/// site: three tests need exactly this list, and a fourth escape shape should
/// be teachable in one place.
///
/// **The vacuity guards live here rather than in one caller**, because every
/// caller is a `for` loop over this list and every one of them passes trivially
/// if it comes back empty. Putting them in the producer means a scanner that
/// stops scanning fails all three at once instead of quietly satisfying two.
fn escaping_tests() -> Vec<String> {
    let escaping: Vec<String> = test_files()
        .into_iter()
        .filter(|(_, source)| escapes(source))
        .map(|(name, _)| name)
        .collect();

    assert!(
        escaping.len() >= 13,
        "expected at least the thirteen files SPEC.md §9 counts, found {}: \
         {escaping:?}. If that is genuinely correct, §9 needs the edit rather \
         than this line",
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
    let lowered = stderr.to_lowercase();
    MARKERS.iter().any(|marker| lowered.contains(marker))
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

/// Every test that reads outside this package is excluded from the package.
///
/// The invariant `SPEC.md` §9 states and could not enforce. A test that escapes
/// cannot compile in an unpacked or vendored copy, because the thing it reaches
/// for is not in the tarball, so shipping one means `cargo test` fails for a
/// reader who did nothing wrong.
///
/// The resolution is directory-wide (`exclude = ["tests/**"]`) rather than
/// per-file, and that is deliberate: thirteen of the nineteen test files escape
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
    assert!(registry_unreachable("Could not connect to server"));
    assert!(registry_unreachable(
        "Caused by: [5] Could not resolve proxy name"
    ));

    // And an unrelated failure is not swallowed.
    assert!(!registry_unreachable(
        "error: failed to parse manifest at `Cargo.toml`"
    ));
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

    let pinned = clean
        .lines()
        .find(|line| line.trim_start().starts_with("vigia-core = {"))
        .and_then(|line| line.split("version = ").nth(1))
        .map(|rest| {
            rest.trim()
                .trim_end_matches(&['}', ' '][..])
                .trim_matches('"')
                .to_owned()
        })
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
    let commands: Vec<&str> = job
        .lines()
        .map(str::trim)
        .filter_map(|line| line.strip_prefix("run: "))
        .collect();
    let publishes: Vec<&&str> = commands
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

    // `--workspace` publishes every member, so what the workspace *holds* is
    // part of what the release ships. A third crate added for any reason, a
    // fixture generator, a proc macro, an experiment, would be published to
    // crates.io permanently on the next tag, and nothing else here would
    // mention it. Two is a deliberate number (`SPEC.md` §6's engine/shell split)
    // rather than an accident, so a third is a decision that should be made
    // rather than discovered.
    let members = toml_array(&repo_file("Cargo.toml"), "members");
    assert_eq!(
        members.len(),
        2,
        "`cargo publish --workspace` ships every member and the workspace now \
         holds {members:?}. Adding one is fine, but it claims a crates.io name \
         forever, so decide it here rather than at the tag"
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
    let output = Command::new(env!("CARGO"))
        .args(["package", "--list", "--allow-dirty", "-p", "vigia"])
        .current_dir(repo_root())
        .output();

    let output = match output {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                registry_unreachable(&stderr),
                "`cargo package --list` failed for a reason that is not the \
                 registry being unreachable, so this is the gate firing rather \
                 than the gate being unavailable:\n{stderr}"
            );
            eprintln!(
                "::warning::SKIPPED the_packaged_artifact_carries_no_tests: the \
                 registry index is unreachable, so this gate proved nothing on \
                 this run. RELEASE-SMOKE.md §1 covers it by hand.\n{stderr}"
            );
            return;
        }
        Err(e) => panic!("could not run cargo at all: {e}"),
    };

    let listed = String::from_utf8_lossy(&output.stdout);
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
        listed.lines().any(|line| line.trim() == "README.md"),
        "README.md is not in the package; `readme` inheritance from the \
         workspace has stopped resolving:\n{listed}"
    );
}
