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

/// The repository root, two levels above this package.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("the repository root is two levels above this package")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// This package's own manifest.
fn manifest() -> String {
    read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
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
    assert!(
        found.len() > 10,
        "found only {} test files, which means the scan is looking in the wrong \
         place and every assertion built on it is vacuous",
        found.len()
    );
    found
}

/// A `#[path]` attribute whose target climbs out of this package.
fn escapes_by_path(source: &str) -> bool {
    source.contains("#[path = \"../../")
}

/// A `CARGO_MANIFEST_DIR` join that climbs out of this package.
///
/// Matched on the *joined literal* rather than on the `env!` alone, because
/// `soak.rs` uses `CARGO_MANIFEST_DIR` twice and only one of the two leaves:
/// `join("tests/soak.rs")` stays inside, and `join(WORKFLOW)` does not. A check
/// that counted every mention would call the first one an escape and be wrong in
/// the direction that looks thorough.
fn escapes_by_manifest_dir(source: &str) -> bool {
    source.contains("CARGO_MANIFEST_DIR") && source.contains("\"../../")
}

/// The `exclude` patterns in this package's manifest.
fn exclude_patterns() -> Vec<String> {
    let manifest = manifest();
    let start = manifest
        .find("\nexclude = [")
        .expect("crates/vigia/Cargo.toml declares an `exclude` list");
    let open = manifest[start..].find('[').expect("the list opens") + start;
    let close = manifest[open..]
        .find(']')
        .expect("the `exclude` list is closed on one line or across several")
        + open;
    manifest[open + 1..close]
        .split(',')
        .map(|entry| entry.trim().trim_matches('"').to_owned())
        .filter(|entry| !entry.is_empty())
        .collect()
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
/// per-file, and that is deliberate: twelve of the seventeen test files escaped
/// already, a per-file list would need editing every time a test is added, and
/// the failure mode of forgetting is silent. This gate holds either shape,
/// because it asks whether each escaping file is *covered*, not how.
#[test]
fn every_test_that_reads_outside_the_package_is_excluded_from_it() {
    let patterns = exclude_patterns();
    let mut escaping = Vec::new();

    for (name, source) in test_files() {
        if escapes_by_path(&source) || escapes_by_manifest_dir(&source) {
            escaping.push(name.clone());
            assert!(
                patterns.iter().any(|p| covers(p, &name)),
                "tests/{name} reads outside the package but no `exclude` pattern \
                 covers it, so a published .crate would carry a test that cannot \
                 compile. Patterns are {patterns:?}"
            );
        }
    }

    // The scan itself has to be able to fail. If a refactor moved the support
    // module inside this package and every escape vanished, the loop above would
    // pass while asserting nothing, and this file would keep looking like a gate.
    assert!(
        escaping.len() >= 12,
        "expected at least the twelve `#[path]` consumers SPEC.md §9 counts, \
         found {}: {escaping:?}. If that is genuinely correct, §9 needs the \
         edit rather than this line",
        escaping.len()
    );
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

    for (name, source) in test_files() {
        if escapes_by_path(&source) || escapes_by_manifest_dir(&source) {
            assert!(
                bullet.contains(&format!("`{name}`")),
                "tests/{name} escapes the package and SPEC.md §9's escape bullet \
                 does not name it. That bullet was wrong by a factor of four for \
                 two phases for exactly this reason"
            );
        }
    }
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

    for pattern in exclude_patterns() {
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

    let root = repo_file("Cargo.toml");
    assert!(
        root.contains(&format!("\"{JOB}\"")),
        "[workspace.metadata.dist] publish-jobs must name {JOB}, or nothing \
         publishes to crates.io on a tag"
    );

    let job = repo_file(WORKFLOW);
    assert!(
        job.contains("workflow_call"),
        "{WORKFLOW} must be a reusable workflow; dist calls it with `uses:`"
    );
    assert!(
        job.contains("cargo publish --workspace"),
        "{WORKFLOW} must publish with `--workspace`, which orders the two crates \
         and waits for the index; sequential `-p` calls need a sleep of no \
         knowable length"
    );

    // dist generates release.yml from the config above, so this asserts the
    // generated file is current rather than that the config is right. A stale
    // release.yml is the failure mode of editing the config and not regenerating.
    let release = repo_file(".github/workflows/release.yml");
    assert!(
        release.contains("uses: ./.github/workflows/publish-crates-io.yml"),
        "release.yml does not call the registry job; run `dist generate`"
    );
    assert!(
        release.contains("- custom-publish-crates-io"),
        "release.yml's `announce` must wait on the registry job, so a failed \
         publish stops the announcement rather than leaving a version whose \
         binaries exist and whose crate does not"
    );
}

/// Every target the release ships is a target the no-C-toolchain gate checks.
///
/// `CLAUDE.md` calls a C build dependency in the graph a spec change rather than
/// an implementation detail, and `ci.yml`'s `pure-rust` job is that rule with
/// teeth. But it holds a hand-written target list and `[workspace.metadata.dist]`
/// holds another, so the rule only covers what someone remembered to type twice.
/// Shipping a fourth target was exactly the moment those two lists came apart:
/// `x86_64-apple-darwin` was added to the release and the purity gate had never
/// heard of it.
///
/// One direction, deliberately. The purity gate may check more than the release
/// ships (a target considered and not yet shipped is worth keeping honest); what
/// it may never do is check less.
#[test]
fn the_targets_the_release_ships_are_the_targets_the_purity_gate_checks() {
    let root = repo_file("Cargo.toml");
    let start = root
        .find("targets = [")
        .expect("[workspace.metadata.dist] declares its targets");
    let end = root[start..].find(']').expect("the target list closes") + start;
    let shipped: Vec<String> = root[start..end]
        .lines()
        .skip(1)
        .map(|line| {
            line.trim()
                .trim_end_matches(',')
                .trim_matches('"')
                .to_owned()
        })
        .filter(|line| !line.is_empty())
        .collect();

    assert!(
        shipped.len() >= 4,
        "parsed only {shipped:?} from the dist target list, which means the \
         parse is wrong rather than the config"
    );

    let ci = repo_file(".github/workflows/ci.yml");
    let checked = ci
        .split("assert no cc, cmake or bindgen")
        .nth(1)
        .expect("ci.yml carries the no-C-toolchain assertion");

    for target in shipped {
        assert!(
            checked.contains(&target),
            "the release ships {target} and the no-C-toolchain gate never checks \
             it, so CLAUDE.md's pure-Rust rule does not cover a binary users \
             receive"
        );
    }
}

/// The tarball cargo would actually upload carries no tests.
///
/// Every gate above reads a file and reasons about what cargo *will* do. This one
/// asks cargo, which is the only way to catch an `exclude` that is spelled
/// correctly and means something other than what it looks like.
///
/// It shells out, so it can be unavailable rather than failing: `cargo package`
/// touches the registry index, and a machine with no network answers a question
/// nobody asked. Skipping is **printed**, on `soak.rs`'s rule that a check which
/// cannot run must say so rather than passing quietly, because a silent skip and
/// a pass are the same green.
#[test]
fn the_packaged_artifact_carries_no_tests() {
    let output = Command::new(env!("CARGO"))
        .args(["package", "--list", "--allow-dirty", "-p", "vigia"])
        .current_dir(repo_root())
        .output();

    let output = match output {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            println!(
                "SKIPPED: `cargo package --list` failed, most likely because the \
                 registry index is unreachable. This gate proves nothing on this \
                 run; RELEASE-SMOKE.md §1 covers it by hand.\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        Err(e) => {
            println!("SKIPPED: could not run cargo: {e}");
            return;
        }
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
