//! What the pane starts as, before anybody presses anything.

use std::fmt;
use std::path::{Path, PathBuf};

use vigia_core::Hidden;

/// Where the view defaults are read from, under the reader's home directory.
pub const CONFIG_FILE: &str = ".config/vigia/config";

/// The state a pane starts in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Move the viewport to what just changed. `f`.
    ///
    /// A key of this file only since `SPEC.md` §11.2 B22: the ruling that made the
    /// config menu the way in also made what a reader flips here worth keeping, and
    /// `f` is a toggle like any other once the writing is theirs.
    pub follow: bool,
    /// Ask for the pinned list beside the diff. `r`.
    pub rail: bool,
    /// Pin the diff to one file. `s`.
    pub single: bool,
    /// Draw the file list alone, with no diff under it. `o`.
    pub overview: bool,
    /// Draw the staged run beside the unstaged one. `a`.
    pub staged: bool,
    /// Wrap a content line too wide for the pane onto the row below. `w`.
    pub wrap: bool,
    /// Draw the rows of the reader's notes under their lines. `c`.
    pub notes: bool,
    /// Draw a file-type icon before every listed path. No gesture; config only.
    pub icons: bool,
    /// Wrap every listed path in an OSC 8 hyperlink to its file. Config only.
    pub links: bool,
    /// Write what the reader flips back into this file. The config menu's own row,
    /// and the only key here that is about the file rather than about the pane.
    pub persist: bool,
    /// Paths to keep out of the pane entirely. No gesture: a pattern is a
    /// decision about a repository rather than about a moment.
    pub hide: Option<Hidden>,
}

/// Every toggle off but follow, the notes and the links, which is the shipped pane.
impl Default for Config {
    fn default() -> Self {
        Self {
            follow: true,
            rail: false,
            single: false,
            overview: false,
            staged: false,
            wrap: false,
            notes: true,
            icons: false,
            links: true,
            persist: false,
            hide: None,
        }
    }
}

/// Every toggle this file accepts, in the order the gestures sheet lists them.
pub const KEYS: [&str; 10] = [
    "follow", "rail", "single", "overview", "staged", "wrap", "notes", "icons", "links", "persist",
];

/// Every setting that takes a value rather than `on` or `off`.
///
/// Apart from [`KEYS`] because a toggle is a pane some key could reach and a
/// valued setting is reachable by no gesture, so a keymap sweep accounts for the
/// first list and never the second.
pub const VALUES: [&str; 1] = ["hide"];

impl Config {
    /// Set `key`, which [`parse`] has already checked is one of [`KEYS`].
    fn set(&mut self, key: &str, on: bool) -> bool {
        match key {
            "follow" => self.follow = on,
            "rail" => self.rail = on,
            "single" => self.single = on,
            "overview" => self.overview = on,
            "staged" => self.staged = on,
            "wrap" => self.wrap = on,
            "notes" => self.notes = on,
            "icons" => self.icons = on,
            "links" => self.links = on,
            "persist" => self.persist = on,
            _ => return false,
        }
        true
    }
}

/// What is wrong with a config file, and which line it is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// The file could not be written, which is only ever reached on the reader's
    /// own gesture with `persist` on.
    Unwritable {
        /// Where the write was aimed.
        path: PathBuf,
        /// What the filesystem said.
        why: String,
    },
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
    /// A `hide` value that is not a regular expression.
    BadPattern {
        /// 1-based.
        line: usize,
        /// The engine's own words, with no sentence around them: the message
        /// below is the sentence.
        why: String,
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
            Self::Unreadable { path, why } => write!(f, "{}: {why}", path.display()),
            // The footer cuts a notice's tail and a path is the long half, so at I6's
            // forty columns the reason is what a refused write has to say first.
            Self::Unwritable { path, why } => write!(f, "{why}: {}", path.display()),
            Self::UnknownKey { line, key } => write!(
                f,
                "line {line}: {key:?} is not a view setting. There are {}: {}",
                KEYS.len() + VALUES.len(),
                KEYS.iter()
                    .chain(VALUES.iter())
                    .copied()
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::BadPattern { line, why } => {
                write!(f, "line {line}: hide is not a pattern: {why}")
            }
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

/// Everything after a key's `=` that is not a comment, trimmed.
///
/// A `#` opens a comment at the start of the value or after whitespace, and
/// nowhere else. What the word-by-word reading this replaced could not do is
/// leave a value alone: rejoining its words normalises runs of spaces, and ` +`
/// and `  +` are different patterns.
fn value_of(after: &str) -> &str {
    let mut opens = true;
    for (at, c) in after.char_indices() {
        if c == '#' && opens {
            return after[..at].trim();
        }
        opens = c.is_whitespace();
    }
    after.trim()
}

/// Parse a config, which is a list of `key = on` lines and nothing else.
///
/// ```text
/// # the pane I want
/// rail     = on    # from 134 columns
/// single   = off
/// staged   = on    # both runs, every session
/// ```
///
/// The theme file's grammar, less what a config has no use for. A theme is a base
/// plus overrides, so two things a theme expresses legitimately are mistakes
/// here: a repeated key ([`ConfigError::RepeatedKey`]) and a value that is
/// nothing but a comment ([`ConfigError::MissingValue`]).
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
    // The line each key was set on, so a repeat can name the one it collides with.
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
        if !KEYS.contains(&key) && !VALUES.contains(&key) {
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

        let value = value_of(value);
        if value.is_empty() {
            return Err(ConfigError::MissingValue { line });
        }

        if VALUES.contains(&key) {
            // One valued key, so the match is on the key rather than on a table.
            config.hide = Some(Hidden::new(value).map_err(|why| ConfigError::BadPattern {
                line,
                why: why.to_string(),
            })?);
            seen.push((key.to_owned(), line));
            continue;
        }

        let on = match value {
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

/// The reader's file with the lines this pane owns brought up to date.
///
/// **Only the value is replaced, never the line.** A line carries the reader's
/// alignment before its `=`, their comment after the value and their own ending,
/// and rebuilding it would reformat the file on every flip while their editor put
/// it back on every save. Comments, blank lines, key order and `hide` survive, and
/// a key the file lacks is appended, so a hand-written file only grows at the end.
#[must_use]
pub fn rewrite(source: &str, config: &Config) -> String {
    // Off the front and back on, for the reason `parse` strips it: U+FEFF is `Cf`
    // rather than `White_Space`, so it survives every trim and lands inside the first
    // key, which would then be unrecognised and appended a second time.
    let mark = source.starts_with('\u{FEFF}');
    let body = if mark {
        &source['\u{FEFF}'.len_utf8()..]
    } else {
        source
    };

    let mut out = String::with_capacity(source.len() + KEYS.len() * 16);
    if mark {
        out.push('\u{FEFF}');
    }
    let mut written: Vec<&str> = Vec::with_capacity(KEYS.len());
    // The first line's ending, so a file written on Windows stays one and an
    // appended key takes what the rest of it has. The first rather than the last
    // because a file carrying both is already inconsistent, and the first is the
    // one the reader's editor goes on using.
    let mut ending = None;

    for piece in body.split_inclusive('\n') {
        let raw = piece.trim_end_matches('\n').trim_end_matches('\r');
        let tail = &piece[raw.len()..];
        if !tail.is_empty() && ending.is_none() {
            ending = Some(tail);
        }
        match owned_key(raw, &written)
            .and_then(|(key, at)| KEYS.iter().find(|name| **name == key).map(|key| (*key, at)))
        {
            Some((key, at)) => {
                written.push(key);
                out.push_str(&raw[..=at]);
                out.push(' ');
                out.push_str(word_for(key, config));
                // What the reader wrote after their value, which is a comment or
                // nothing.
                if let Some(said) = comment_in(&raw[at + 1..]) {
                    out.push(' ');
                    out.push_str(said.trim_end());
                }
            }
            None => out.push_str(raw),
        }
        out.push_str(tail);
    }

    let missing: Vec<String> = KEYS
        .iter()
        .filter(|key| !written.contains(key))
        .map(|key| format!("{key} = {}", word_for(key, config)))
        .collect();
    if !missing.is_empty() {
        let ending = ending.unwrap_or("\n");
        // The body rather than the output, or a file that is only a byte order mark
        // takes a blank line it never had, and *an* ending rather than this one,
        // because a file carrying both ends with whichever its last line used.
        if !body.is_empty() && !out.ends_with('\n') {
            out.push_str(ending);
        }
        for line in missing {
            out.push_str(&line);
            out.push_str(ending);
        }
    }
    out
}
/// The key one line sets and the offset of its `=`, where it sets one this pane
/// owns and has not written yet.
fn owned_key<'a>(raw: &'a str, written: &[&str]) -> Option<(&'a str, usize)> {
    let text = raw.trim_start();
    if text.is_empty() || text.starts_with('#') {
        return None;
    }
    let at = raw.find('=')?;
    let key = raw[..at].trim();
    (KEYS.contains(&key) && !written.contains(&key)).then_some((key, at))
}

/// The comment a value carries, from its `#` to the line's end.
fn comment_in(after: &str) -> Option<&str> {
    let mut opens = true;
    for (at, c) in after.char_indices() {
        if c == '#' && opens {
            return Some(&after[at..]);
        }
        opens = c.is_whitespace();
    }
    None
}

/// What a key's value spells, given where it stands.
fn word_for(key: &str, config: &Config) -> &'static str {
    let on = match key {
        "follow" => config.follow,
        "rail" => config.rail,
        "single" => config.single,
        "overview" => config.overview,
        "staged" => config.staged,
        "wrap" => config.wrap,
        "notes" => config.notes,
        "icons" => config.icons,
        "links" => config.links,
        "persist" => config.persist,
        // Unreachable through `owned_key`, which filters on `KEYS`; a valued
        // setting reaches no gesture and so is never rewritten.
        _ => return "off",
    };
    if on { "on" } else { "off" }
}

/// Where a write to `path` has to land: what a link points at, never the link.
/// `canonicalize` refuses a path with anything missing in it, which is a first save
/// and also a link laid down before what it names exists, and that is still a link.
fn landing(path: &Path) -> PathBuf {
    if let Ok(whole) = std::fs::canonicalize(path) {
        return whole;
    }
    let Ok(to) = std::fs::read_link(path) else {
        return path.to_owned();
    };
    match path.parent() {
        Some(dir) if to.is_relative() => dir.join(to),
        _ => to,
    }
}

/// Write `config` back into the reader's own file.
///
/// Temp-and-rename, the note store's shape and for its reason: a reader who quits
/// mid-write has a whole file either way, and the id in the temp keeps two panes apart.
///
/// **A file that no longer parses is refused rather than rewritten**, because it
/// means a reader is editing it by hand or it already holds what the next launch
/// will refuse, and writing over either decides for them what their file says.
/// What is left is two panes writing in turn, where the last flip wins: closing
/// that needs a lock, and a lock is a file nobody asked this program to write.
///
/// # Errors
///
/// The directory cannot be made, the file cannot be read or no longer parses, or
/// the write cannot land.
pub fn save(path: &Path, config: &Config) -> Result<(), ConfigError> {
    let refuse = |why: std::io::Error| ConfigError::Unwritable {
        path: path.to_owned(),
        why: why.to_string(),
    };
    let target = landing(path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(refuse)?;
    }
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(why) => return Err(refuse(why)),
    };
    parse(&source)?;
    let temp = target.with_extension(format!("writing-{}", std::process::id()));
    let wipe = |why: std::io::Error| {
        let _ = std::fs::remove_file(&temp);
        refuse(why)
    };
    std::fs::write(&temp, rewrite(&source, config)).map_err(wipe)?;
    std::fs::rename(&temp, &target).map_err(wipe)
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
