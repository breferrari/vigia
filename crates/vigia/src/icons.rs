//! Which Nerd Font glyph names a file's type, when a reader has asked for one.
//!
//! `SPEC.md` §11.2 B18 ([#323](https://github.com/breferrari/vigia/issues/323)),
//! and the shape is the field's (`docs/research/318-drawing-vocabulary.md`
//! §1.3): lazygit ships icons off by default behind a version key, yazi ships
//! them theme-driven, and every serious tool keeps a clean glyphless mode. Here
//! the switch is the config file's `icons` key, the default is off, and off
//! means byte-identical output, which `tests/render.rs` holds as a buffer
//! comparison rather than a promise.
//!
//! **Every codepoint below was verified against the stock JetBrainsMono Nerd
//! Font on 2026-08-25** (`fc-list :charset=<cp>`, the sweep is in the research
//! dossier §3.1), which is the #316 discipline: a PUA glyph is a font bet, so
//! nothing lands here on a cheat-sheet's word. That is also why the table is
//! short: an icon that might be tofu is worse than the letter the row already
//! carries, and the generic mark covers everything the table does not.
//!
//! **When icons are on, every row gets one.** A table miss draws
//! [`GENERIC`], never nothing, because a mixed list would put paths at two
//! origins and re-create exactly the sliding-columns defect
//! [#77](https://github.com/breferrari/vigia/issues/77) closed.
//!
//! **The icon takes the row's own ink**, the recency ladder included, rather
//! than a per-type hue: §5.3 spends colour by role, thirty new roles is not a
//! vocabulary a reader can learn, and an icon that dims with its file says one
//! more true thing at zero cost.

/// The mark for a file the table has no entry for.
pub const GENERIC: char = '\u{f016}';

/// The glyph for `path`, by its extension or well-known name.
pub fn icon_of(path: &str) -> char {
    let name = path.rsplit('/').next().unwrap_or(path);
    let lower = name.to_ascii_lowercase();
    if lower.starts_with(".git") {
        return '\u{e702}';
    }
    let extension = lower.rsplit('.').next().unwrap_or_default();
    match extension {
        "rs" => '\u{e7a8}',
        "js" | "mjs" | "cjs" => '\u{e74e}',
        "ts" | "tsx" | "jsx" => '\u{e628}',
        "py" => '\u{e73c}',
        "rb" => '\u{e739}',
        "java" => '\u{e738}',
        "sh" | "bash" | "zsh" | "fish" => '\u{f489}',
        "md" | "markdown" => '\u{f48a}',
        "json" => '\u{e60b}',
        "toml" | "yaml" | "yml" | "ini" | "conf" => '\u{e615}',
        "lock" => '\u{e60b}',
        _ => GENERIC,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_known_extension_names_its_glyph_and_an_unknown_one_the_generic() {
        assert_eq!(icon_of("src/render/frame.rs"), '\u{e7a8}');
        assert_eq!(icon_of("docs/THEME.md"), '\u{f48a}');
        assert_eq!(icon_of(".gitignore"), '\u{e702}');
        assert_eq!(icon_of("assets/blob.bin"), GENERIC);
        assert_eq!(icon_of("no-extension"), GENERIC);
    }
}
