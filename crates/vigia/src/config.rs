//! What the pane starts as, before anybody presses anything.
//!
//! `SPEC.md` §11.2 **B6**, amended 2026-08-25 by
//! [#306](https://github.com/breferrari/vigia/issues/306). Three keys, in a file
//! beside the theme: `masthead`, `rail`, `single` and `staged`, the toggles that decide what
//! the body is made of. All three are off without a file, which is every version
//! of this tool before the amendment, so a reader who has written nothing sees
//! exactly what they saw yesterday.
//!
//! ## Why this is a file and not a variable
//!
//! B6 splits settings by what they are *about*. A preference about **you** goes in
//! a file, because it should follow you into every shell; a fact about the
//! **terminal in front of you** stays a variable, because one machine has three
//! terminals in an afternoon and a file would give all three the same wrong
//! answer. Which toggles you want is the first kind. Whether this pane is wide
//! enough to honour the rail is the second, and it is not configured at all: it is
//! measured, every frame.
//!
//! ## Why this is a second file and not three more keys in the theme
//!
//! [`crate::theme::from_env`] resolves `VIGIA_THEME` first and a built-in name
//! wins outright, so **`VIGIA_THEME=dark` never opens the theme file at all**.
//! View keys living beside the colours would therefore be discarded, silently, by
//! a reader naming a palette for one session, on a gesture with nothing to do with
//! the settings it lost. That is the same failure [`parse`] refuses unknown keys
//! to avoid, and it is a defect rather than a matter of taste, which is why it
//! decided the shape. `RULINGS.md` carries the rest.
//!
//! What the two files share is everything that costs something: one format, one
//! discovery rule, one error path, and one order at startup. What they do not
//! share is a subject.
//!
//! ## What is deliberately absent
//!
//! **`follow` is not a key.** I5 is *correct with zero interaction, auto-follows
//! the newest change*, which is a promise about the program; a file able to turn
//! following off would quietly make it a promise about one reader's configuration.
//! `f` says otherwise for a session, which is where a session's choice belongs.
//!
//! **And no environment variable joins these.** A variable is how you say *not
//! this time* without editing anything, which is `VIGIA_THEME`'s whole job. Here
//! that sentence is already spoken by `m`, `r` and `s`, one press each and named
//! on the gestures sheet, so a variable would be a second spelling of something
//! the pane says better. B6's count of one is untouched.

use std::fmt;
use std::path::{Path, PathBuf};

/// Where the view defaults are read from, under the reader's home directory.
///
/// Beside [`crate::theme::THEME_FILE`] and resolved by the same rule, which is
/// `HOME` then `USERPROFILE` with each candidate checked for emptiness before the
/// next is tried. One rule rather than one per platform: no XDG matrix, no
/// `%APPDATA%` special case, no discovery crate.
pub const CONFIG_FILE: &str = ".config/vigia/config";

/// The state a pane starts in.
///
/// **`Default` is every toggle off, and that is the shipped pane rather than a
/// neutral-looking choice.** A reader with no file gets what every version before
/// [#306](https://github.com/breferrari/vigia/issues/306) drew, which is what
/// makes the amendment additive: the file is a way to say something, never a
/// requirement to say it.
///
/// Four fields rather than a map, because the set is closed by a ruling and a
/// map would let [`parse`] accept a key nothing reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// Draw the churn band at the top. `m`.
    pub masthead: bool,
    /// Ask for the pinned list beside the diff. `r`.
    ///
    /// **A request rather than a layout**, which is §11.2 B14 unchanged: a pane
    /// under 134 columns draws no rail whatever this says, and the request is kept
    /// rather than cleared, so widening such a pane produces the rail rather than
    /// the question. What this file sets is `Chrome::rail`; what the pane can give
    /// is `Body::rail`.
    pub rail: bool,
    /// Pin the diff to one file. `s`.
    pub single: bool,
    /// Draw the staged run beside the unstaged one. `a`.
    ///
    /// **§11.2 B17** ([#313](https://github.com/breferrari/vigia/issues/313)), and
    /// it is here because this file's set is *defined* as the gestures sheet's
    /// `view` section less `follow`: the section gained a fourth row, so leaving
    /// this out would have made that rule false rather than merely leaving a key
    /// unavailable.
    ///
    /// **It answers nothing that [#50](https://github.com/breferrari/vigia/issues/50)
    /// asks.** That row is still open and still on a week of real use: it decides
    /// which way the toggle *ships*, and the shipped default is unchanged and off.
    /// This lets a reader who has already decided for themselves stop pressing `a`
    /// every session, which is what every key in this file does.
    pub staged: bool,
    /// Draw a file-type icon before every listed path. No gesture; config only.
    ///
    /// **The first key here with no key on the keyboard, and that is a
    /// deliberate amendment to this set's definition** (`SPEC.md` §11.2 B18,
    /// [#323](https://github.com/breferrari/vigia/issues/323)). The set used to
    /// be *the gestures sheet's view section less `follow`*; it is now that,
    /// plus the appearance keys B18 added, because an appearance preference is
    /// exactly what a config file is for and exactly what a session gesture is
    /// not: nobody re-decides their icons per pane. Off by default, and off is
    /// byte-identical to every version before it, which `tests/render.rs`
    /// holds as a buffer comparison.
    pub icons: bool,
    /// Wrap every listed path in an OSC 8 hyperlink to its file. Config only.
    ///
    /// **The first key whose default is on**, and the reasoning is B18's
    /// ([#326](https://github.com/breferrari/vigia/issues/326)): the 2026
    /// support matrix shows OSC 8 degrading silently everywhere it is not
    /// understood, so there is nothing to protect a reader from and the key
    /// exists to turn a nicety off, not to discover it. That inverts this
    /// struct's `Default`, which is now written by hand and says so.
    pub links: bool,
}

/// Every toggle off and the links on, which is the shipped pane.
///
/// **Hand-written because `links` inverted the rule** the derive encoded: the
/// derived `Default` was "everything off equals the pane every version drew",
/// and `links` defaults on for its own field's reason. `App::new` and this must
/// stay one answer; `tests/config.rs` pins the pair.
impl Default for Config {
    fn default() -> Self {
        Self {
            masthead: false,
            rail: false,
            single: false,
            staged: false,
            icons: false,
            links: true,
        }
    }
}

/// Every key this file accepts, in the order the gestures sheet lists them.
///
/// **This is what [`parse`] admits a key by**, not merely what the error message
/// prints, and the two were separate in the first draft: the list was decoration
/// beside a `match` that did the real work, so they could drift and the message
/// would have advertised a set the parser did not accept.
///
/// **It is not a compile-time guarantee and an earlier docblock claimed it was.**
/// [`Config::set`] matches on a `&str` with a fallback arm, so a fourth field on
/// [`Config`] compiles perfectly well with no key and no entry here. Two things
/// tie them instead, and the first draft had only the weaker one:
///
/// - **A key here that [`Config::set`] does not take is an error at parse time**,
///   because [`parse`] reads that function's return rather than discarding it.
///   Without that, adding a name here and forgetting the arm gave a file whose key
///   parsed and did nothing.
/// - **A field with no key here is caught by
///   `tests/config.rs::every_key_is_a_field_and_every_field_is_a_key`**, which
///   sets every key in this list and compares against a **struct literal naming
///   every field** — and a literal is where a new field does stop the build.
///
/// That is a gate and a runtime check rather than the type system, and saying so
/// is the difference between a check and a claim that suppresses one.
pub const KEYS: [&str; 6] = ["masthead", "rail", "single", "staged", "icons", "links"];

impl Config {
    /// Set `key`, which [`parse`] has already checked is one of [`KEYS`].
    ///
    /// **The fallback arm is unreachable today and its `bool` is read anyway**,
    /// because [`KEYS`] and this function are two lists and two lists drift.
    /// [`parse`] admits a key by that list and applies it here, so while the two
    /// agree this returns `true` every time; when they stop agreeing, `parse`
    /// refuses the key and a reader sees an error rather than a setting that
    /// quietly does nothing.
    ///
    /// **Two earlier docblocks got this wrong in opposite directions**, which is
    /// worth leaving on the page. The first called the `match` exhaustive the way
    /// [`crate::input::Action::needs_height`] is; that comparison is wrong,
    /// because the other one matches an **enum** and this matches a `&str`. The
    /// second overcorrected to *reachable*, which contradicted [`parse`]'s own
    /// paragraph in the same file. Unreachable-but-read is the accurate one, and
    /// it is why a mutation deleting the check survives the suite.
    fn set(&mut self, key: &str, on: bool) -> bool {
        match key {
            "masthead" => self.masthead = on,
            "rail" => self.rail = on,
            "single" => self.single = on,
            "staged" => self.staged = on,
            "icons" => self.icons = on,
            "links" => self.links = on,
            _ => return false,
        }
        true
    }
}

/// What is wrong with a config file, and which line it is on.
///
/// Shaped after [`crate::theme::ThemeError`] deliberately: the two files share a
/// grammar, so they should fail in the same words, and a reader who has met one
/// error should not have to learn a second vocabulary. **[`Self::RepeatedKey`] is
/// the one word they do not share**, and its own docblock says why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// The file exists and could not be read.
    ///
    /// **Absent is not this**, and the distinction is the theme file's: nobody has
    /// to have a config, but a reader who wrote one and got the defaults silently
    /// would have no way to find out why.
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
    ///
    /// `rail = # oops` is this rather than a value of `#`: no value in this file
    /// begins with a `#`, so a comment there means the reader wrote no value. The
    /// theme file answers differently because a colour does begin with one; see
    /// [`parse`].
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
    ///
    /// **Refused where the theme file's ordinary keys are not, and the difference
    /// is `base` rather than strictness.** A theme is a base plus overrides, so a
    /// later line legitimately replaces an earlier one and last-wins is the
    /// grammar working. This file has no base, so no line can be an intentional
    /// override of another and every repeat is a mistake. `ThemeError::RepeatedBase`
    /// is the same reasoning applied to the one theme key that has no base above
    /// it.
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
/// **The theme file's grammar, with three deliberate divergences**, all of them
/// the same trade in different clothes: this file has no `base` and no value that
/// begins with `#`, so several things a theme legitimately expresses are mistakes
/// here. They are [`ConfigError::RepeatedKey`], a value with a trailing token, and
/// the comment-only value below. A byte order mark
/// is stripped, because U+FEFF is `Cf` rather than `White_Space` and survives
/// every trim, landing inside the first key: a file saved by Notepad would
/// otherwise stop the shell with an error naming an invisible byte. A comment is
/// recognised on the trimmed *line* for a blank-or-comment line, and stripped from
/// the *value* so `rail = on # from 134` works. And an unknown key is **refused
/// rather than ignored**, because a silently dropped key is a setting that does
/// nothing, and "it was discarded" is the one explanation a reader cannot arrive
/// at by looking at their screen.
///
/// **The third is a value that is nothing but a comment.** `theme::words_of` keeps
/// a bare `#` as a token, because a theme value legitimately begins with one and
/// `added = #3fb950` has to parse. No value here begins with `#`, so
/// `rail = # oops` is a key with nothing after its `=` rather than a key whose
/// value is `#`, and it reports [`ConfigError::MissingValue`] accordingly.
///
/// The other two are the same shape. A theme is a base plus overrides, so a
/// repeated key is a later line replacing an earlier one and `theme::parse` takes
/// the last; here there is no base, so a repeat is a mistake and
/// [`ConfigError::RepeatedKey`] says so. And a theme value is several words, so
/// `theme::style_of` reads them all; here a value is one word, so a second is a
/// typo rather than more value.
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

        // **The key is judged before its value, which is the theme parser's order
        // and was not this one's.** Written the other way round, `sidebar = yes`
        // reported that `sidebar` is *"neither `on` nor `off`"*, which asserts
        // that `sidebar` is a setting; and `sidebar =` reported a missing value
        // and named no key at all. The first thing wrong with a line is the thing
        // to say about it.
        if !KEYS.contains(&key) {
            return Err(ConfigError::UnknownKey {
                line,
                key: key.to_owned(),
            });
        }

        // **And a repeat is judged before the value too**, so `rail = on` followed
        // by `rail = yes` reports the repeat rather than the typo: the repeat is
        // the reason the line should not be there at all.
        if let Some((_, first)) = seen.iter().find(|(name, _)| name == key) {
            return Err(ConfigError::RepeatedKey {
                line,
                key: key.to_owned(),
                first: *first,
            });
        }

        // **The whole value, not its first word.** A `#` ends it, so
        // `rail = on # from 134` works; everything before that `#` has to be the
        // value. Taking only the first token accepted `rail = on off` and
        // `rail=on=off` by discarding what it did not understand, which is the
        // silence this parser refuses unknown keys to avoid, one field over.
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

        // **The return is read, and discarding it was the hole round one left.**
        // The check above admits a key by [`KEYS`]; this applies it by
        // [`Config::set`]; and until the `bool` was read, a name in the first and
        // not the second parsed to `Ok` and set nothing. A key that did nothing,
        // silently, is exactly what refusing unknown keys exists to prevent, so
        // the drift produced the failure the whole grammar is designed against.
        // `theme::parse` has always read its own `set` for this reason.
        //
        // **A mutation deleting this branch survives the suite, and that is
        // accurate rather than a gap.** The two lists agree today, so `set` always
        // returns `true` and the branch is unreachable: it is defence against an
        // edit nobody has made. What it buys when that edit happens is the
        // difference between a reader seeing an error and a reader seeing nothing,
        // because `tests/config.rs::every_key_is_a_field_and_every_field_is_a_key`
        // catches the drift in CI either way.
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
pub fn load(path: &Path) -> Result<Config, ConfigError> {
    let source = std::fs::read_to_string(path).map_err(|why| ConfigError::Unreadable {
        path: path.to_owned(),
        why: why.to_string(),
    })?;
    parse(&source)
}

/// The view defaults this process should start with.
///
/// **Absent is not an error and unreadable is**, which is [`crate::theme`]'s
/// distinction for its reason: nobody has to have a file, and a reader who wrote
/// one and got the defaults silently would have no way to find out why.
///
/// `lookup` rather than `std::env::var` directly, so a test can place a home
/// directory without touching the process environment. That is the theme's shape
/// and it is what makes both testable without a lock.
pub fn from_env(lookup: impl Fn(&str) -> Option<String>) -> Result<Config, ConfigError> {
    match crate::theme::home_file(CONFIG_FILE, &lookup).filter(|path| path.is_file()) {
        Some(path) => load(&path),
        None => Ok(Config::default()),
    }
}
