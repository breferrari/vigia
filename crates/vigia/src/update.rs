//! Whether a newer `vigia` has been published, asked once at startup.
//!
//! `SPEC.md` §11.1: one request, on a thread, and a footer notice when the
//! answer is higher than this binary. Current, offline, refused, slow,
//! unparseable and an architecture the TLS provider does not compile on are one
//! outcome between them, which is silence.

use std::fmt;

use crate::colour::override_of;

/// Environment variable that declines the check.
pub const UPDATE_VAR: &str = "VIGIA_UPDATE";

/// A [`UPDATE_VAR`] this does not understand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateError {
    /// What was found in the variable.
    pub value: String,
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{UPDATE_VAR}: {:?} is not one of auto, off", self.value)
    }
}

impl std::error::Error for UpdateError {}

/// Whether the environment asks for the check.
///
/// # Errors
///
/// The variable holds something that is neither `auto` nor `off`.
pub fn wanted(lookup: impl Fn(&str) -> Option<String>) -> Result<bool, UpdateError> {
    // Through `override_of` for the set-but-empty rule, which is a PowerShell
    // gotcha the colour ladder already paid for.
    let Some((raw, normalised)) = override_of(&lookup, UPDATE_VAR) else {
        return Ok(true);
    };
    match normalised.as_str() {
        "auto" => Ok(true),
        "off" => Ok(false),
        _ => Err(UpdateError { value: raw }),
    }
}

/// The newest stable release the answer names.
pub fn version_in(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    // Not `newest_version`, which names a prerelease whenever one is published.
    value["crate"]["max_stable_version"]
        .as_str()
        .map(str::to_owned)
}

/// Whether `remote` is a later release than `local`.
pub fn newer(remote: &str, local: &str) -> bool {
    match (triple(remote), triple(local)) {
        (Some(remote), Some(local)) => remote > local,
        _ => false,
    }
}

/// A version read as exactly three numbers, and nothing else.
///
/// The field this reads carries no prerelease or build metadata by
/// construction, so a string semver would accept and this will not is a sign
/// that something upstream changed rather than a shape to guess at.
fn triple(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next()?.parse::<u64>().ok()?;
    let patch = parts.next()?.parse::<u64>().ok()?;
    parts.next().is_none().then_some((major, minor, patch))
}

/// Ask `check` on a thread, and hand whatever it answers to `say`.
///
/// Detached rather than joined, which is what keeps I7 whole: the request is
/// 612ms measured against a first paint budget of 50ms, so the caller has to be
/// back on screen long before this can answer.
pub fn watch(
    check: impl FnOnce() -> Option<String> + Send + 'static,
    say: impl FnOnce(String) + Send + 'static,
) {
    std::thread::spawn(move || {
        if let Some(version) = check() {
            say(version);
        }
    });
}

/// The newer version, when the registry has one and this binary is not it.
pub fn check(current: &str) -> Option<String> {
    let published = version_in(&fetch()?)?;
    newer(&published, current).then_some(published)
}

/// Where the newest published version is asked for.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
const ENDPOINT: &str = "https://crates.io/api/v1/crates/vigia";

/// How long the request may run before its answer is about a pane nobody is
/// still looking at.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
const FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// How much of the answer is read.
///
/// The endpoint returns 71KB at forty-four published versions, so this is an
/// order above what it sends and far below a size a monitor would feel.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
const BODY_LIMIT: u64 = 1024 * 1024;

/// Ask the registry, or answer nothing.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn fetch() -> Option<String> {
    if !provider_runs_here() {
        return None;
    }
    // Process-wide, and an already-installed one is as good as this one.
    let _ = rustls_graviola::default_provider().install_default();

    let agent: ureq::Agent = ureq::Agent::config_builder()
        // The registry asks a client to name itself and say where to complain.
        .user_agent(concat!(
            "vigia/",
            env!("CARGO_PKG_VERSION"),
            " (",
            env!("CARGO_PKG_REPOSITORY"),
            ")"
        ))
        .timeout_global(Some(FETCH_TIMEOUT))
        .build()
        .into();

    agent
        .get(ENDPOINT)
        .call()
        .ok()?
        .body_mut()
        .with_config()
        .limit(BODY_LIMIT)
        .read_to_string()
        .ok()
}

/// Nothing to ask on an architecture the provider will not compile on.
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn fetch() -> Option<String> {
    None
}

/// Whether this CPU has everything the TLS provider asserts on.
///
/// The provider asserts rather than degrades, the release profile aborts rather
/// than unwinds, and the thread this runs on is the monitor's. So a machine
/// below its floor, which its own source puts at roughly 2013, has to lose the
/// check here rather than lose the pane there. The list is read from the
/// provider's source, not guessed.
#[cfg(target_arch = "x86_64")]
pub fn provider_runs_here() -> bool {
    is_x86_feature_detected!("aes")
        && is_x86_feature_detected!("pclmulqdq")
        && is_x86_feature_detected!("bmi1")
        && is_x86_feature_detected!("adx")
        && is_x86_feature_detected!("avx")
        && is_x86_feature_detected!("avx2")
}

/// Whether this CPU has everything the TLS provider asserts on.
///
/// The x86_64 arm of this carries why it exists.
#[cfg(target_arch = "aarch64")]
pub fn provider_runs_here() -> bool {
    use std::arch::is_aarch64_feature_detected;

    is_aarch64_feature_detected!("neon")
        && is_aarch64_feature_detected!("aes")
        && is_aarch64_feature_detected!("pmull")
        && is_aarch64_feature_detected!("sha2")
}
