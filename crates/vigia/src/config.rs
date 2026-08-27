//! What the pane starts as, before anybody presses anything.

use std::fmt;
use std::path::{Path, PathBuf};

/// Where the view defaults are read from, under the reader's home directory.
pub const CONFIG_FILE: &str = ".config/vigia/config";

/// The state a pane starts in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// Draw the churn band at the top. `m`.
    pub masthead: bool,
    /// Ask for the pinned list beside the diff. `r`.
    pub rail: bool,
    /// Pin the diff to one file. `s`.
    pub single: bool,
    /// Draw the staged run beside the unstaged one. `a`.
    pub staged: bool,
    /// Wrap a content line too wide for the pane onto the row below. `w`.
    pub wrap: bool,
    /// Draw a file-type icon before every listed path. No gesture; config only.
    pub icons: bool,
    /// Wrap every listed path in an OSC 8 hyperlink to its file. Config only.
    pub links: bool,
}

/// Every toggle off and the links on, which is the shipped pane.
impl Default for Config {
    fn default() -> Self {
        Self {
            masthead: false,
            rail: false,
            single: false,
            staged: false,
            wrap: false,
            icons: false,
            links: true,
        }
    }
}

/// Every key this file accepts, in the order the gestures sheet lists them.
pub const KEYS: [&str; 7] = [
    "masthead", "rail", "single", "staged", "wrap", "icons", "links",
];

impl Config {
    /// Set `key`, which [`parse`] has already checked is one of [`KEYS`].
    fn set(&mut self, key: &str, on: bool) -> bool {
        match key {
            "masthead" => self.masthead = on,
            "rail" => self.rail = on,
            "single" => self.single = on,
            "staged" => self.staged = on,
            "wrap" => self.wrap = on,
            "icons" => self.icons = on,
            "links" => self.links = on,
            _ => return false,
        }
        true
    }
}

/// What is wrong with a config file, and which line it is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// The file exists and could not be read.
    Unreadable {
        /// Where it was looked for.
        path: PathBuf,
        /// What the filesystem said.
        why: String,
    },
    /// A key this file does not have.
    UnknownKey {
        /// 1-based, as a reader's editor counts.
        line: usize,
        /// What they wrote.
        key: String,
    },
    /// A value that is neither `on` nor `off`.
    UnknownValue {
        /// 1-based.
        line: usize,
        /// The key it was given to, so the message can name both.
        key: String,
        /// What they wrote.
        value: String,
    },
    /// A key with nothing after its `=`, or nothing but a comment.
    MissingValue {
        /// 1-based.
        line: usize,
    },
    /// A line that is not a comment and has no `=` in it.
    MissingSeparator {
        /// 1-based.
        line: usize,
        /// The line, so the message can quote it back.
        text: String,
    },
    /// The same key twice.
    RepeatedKey {
        /// 1-based, the second occurrence.
        line: usize,
        /// The key, so the message can name it.
        key: String,
        /// Where it was set before, so a reader can find the other one.
        first: usize,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { path, why } => {
                write!(f, "{}: {why}", path.display())
            }
            Self::UnknownKey { line, key } => write!(
                f,
                "line {line}: {key:?} is not a view setting. There are three: {}",
                KEYS.join(", ")
            ),
            Self::UnknownValue { line, key, value } => write!(
                f,
                "line {line}: {key} is {value:?}, which is neither `on` nor `off`"
            ),
            Self::MissingValue { line } => write!(
                f,
                "line {line}: this key has nothing after its `=`. Write `on` or `off`"
            ),
            Self::MissingSeparator { line, text } => {
                write!(f, "line {line}: {text:?} has no `=` in it")
            }
            Self::RepeatedKey { line, key, first } => write!(
                f,
                "line {line}: {key} was already set on line {first}. Remove one of \
                 them rather than leaving which one wins to the reader"
            ),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Parse a config, which is a list of `key = on` lines and nothing else.
///
/// ```text
/// # the pane I want
/// masthead = on
/// rail     = on    # from 134 columns
/// single   = off
/// staged   = on    # both runs, every session
/// ```
///
/// The theme file's grammar, less what a config has no use for. A theme is a base
/// plus overrides and its values are several words; this has no base and its
/// values are one word, so three things a theme expresses legitimately are
/// mistakes here: a repeated key ([`ConfigError::RepeatedKey`]), a trailing token,
/// and a value that is nothing but a comment ([`ConfigError::MissingValue`] —
/// `theme::words_of` keeps a bare `#` because `added = #3fb950` has to parse, and
/// no value here begins with one).
///
/// An unknown key is refused rather than ignored: a silently dropped key is a
/// setting that does nothing, which is the one explanation a reader cannot reach
/// by looking at their screen.
///
/// A byte order mark is stripped. U+FEFF is `Cf` rather than `White_Space`, so it
/// survives every trim and lands inside the first key, and a file saved by Notepad
/// would otherwise stop the shell with an error naming an invisible byte.
///
/// # Errors
///
/// A line is not `key = on`: an unknown key or value, a missing value or separator, a
/// repeated key, or a value carrying a trailing token.
pub fn parse(source: &str) -> Result<Config, ConfigError> {
    let source = source.strip_prefix('\u{FEFF}').unwrap_or(source);

    let mut config = Config::default();
    // Where each key was set, so a repeat can name the line it collides with
    // rather than only its own.
    let mut seen: Vec<(String, usize)> = Vec::new();

    for (index, raw) in source.lines().enumerate() {
        let line = index + 1;
        let text = raw.trim();
        // Checked on the trimmed line rather than after any `#` handling: a key
        // never begins with `#`, so a leading one is always a comment.
        if text.is_empty() || text.starts_with('#') {
            continue;
        }

        let Some((key, value)) = text.split_once('=') else {
            return Err(ConfigError::MissingSeparator {
                line,
                text: text.to_owned(),
            });
        };
        let key = key.trim();

        // The key is judged before its value, which is the theme parser's order and was
        // not this one's.
        if !KEYS.contains(&key) {
            return Err(ConfigError::UnknownKey {
                line,
                key: key.to_owned(),
            });
        }

        // And a repeat is judged before the value too, so `rail = on` followed
        // by `rail = yes` reports the repeat rather than the typo: the repeat is
        // the reason the line should not be there at all.
        if let Some((_, first)) = seen.iter().find(|(name, _)| name == key) {
            return Err(ConfigError::RepeatedKey {
                line,
                key: key.to_owned(),
                first: *first,
            });
        }

        // The whole value, not its first word. A `#` ends it, so `rail = on # from 134`
        // works; everything before that `#` has to be the value.
        let value: Vec<&str> = value
            .split_whitespace()
            .take_while(|word| !word.starts_with('#'))
            .collect();
        if value.is_empty() {
            return Err(ConfigError::MissingValue { line });
        }
        let value = value.join(" ");

        let on = match value.as_str() {
            "on" => true,
            "off" => false,
            other => {
                return Err(ConfigError::UnknownValue {
                    line,
                    key: key.to_owned(),
                    value: other.to_owned(),
                });
            }
        };

        // The return is read, and discarding it is the hole.
        if !config.set(key, on) {
            return Err(ConfigError::UnknownKey {
                line,
                key: key.to_owned(),
            });
        }
        seen.push((key.to_owned(), line));
    }

    Ok(config)
}

/// Read and parse a config file.
///
/// # Errors
///
/// The file cannot be read, or it does not parse.
pub fn load(path: &Path) -> Result<Config, ConfigError> {
    let source = std::fs::read_to_string(path).map_err(|why| ConfigError::Unreadable {
        path: path.to_owned(),
        why: why.to_string(),
    })?;
    parse(&source)
}

/// The view defaults this process should start with.
///
/// # Errors
///
/// The configuration file named by the environment cannot be read, or does not parse.
pub fn from_env(lookup: impl Fn(&str) -> Option<String>) -> Result<Config, ConfigError> {
    match crate::theme::home_file(CONFIG_FILE, &lookup).filter(|path| path.is_file()) {
        Some(path) => load(&path),
        None => Ok(Config::default()),
    }
}
