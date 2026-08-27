//! Which Nerd Font glyph names a file's type, when a reader has asked for one.

/// The mark for a file the table has no entry for.
pub const GENERIC: char = '\u{f016}';

/// The glyph for `path`, by its extension or well-known name.
pub fn icon_of(path: &str) -> char {
    let name = path.rsplit('/').next().unwrap_or(path);
    if name
        .get(..4)
        .is_some_and(|p| p.eq_ignore_ascii_case(".git"))
    {
        return '\u{e702}';
    }
    let Some(extension) = std::path::Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
    else {
        return GENERIC;
    };
    // Lowercased into a stack buffer: the longest key is `markdown`, eight
    // bytes, and anything longer is the generic mark before any copying.
    let mut lower = [0u8; 8];
    if extension.len() > lower.len() || !extension.is_ascii() {
        return GENERIC;
    }
    lower[..extension.len()].copy_from_slice(extension.as_bytes());
    lower[..extension.len()].make_ascii_lowercase();
    match &lower[..extension.len()] {
        b"rs" => '\u{e7a8}',
        b"js" | b"mjs" | b"cjs" => '\u{e74e}',
        b"ts" | b"tsx" | b"jsx" => '\u{e628}',
        b"py" => '\u{e73c}',
        b"rb" => '\u{e739}',
        b"java" => '\u{e738}',
        b"sh" | b"bash" | b"zsh" | b"fish" => '\u{f489}',
        b"md" | b"markdown" => '\u{f48a}',
        b"json" | b"lock" => '\u{e60b}',
        b"toml" | b"yaml" | b"yml" | b"ini" | b"conf" => '\u{e615}',
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
        // A file *named* like an extension has none, which is the rule this
        // shares with the core's own `Path::extension` use.
        assert_eq!(icon_of("src/rs"), GENERIC);
        assert_eq!(icon_of("SRC/MAIN.RS"), '\u{e7a8}', "case folds");
    }
}
