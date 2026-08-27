//! `docs/THEME.md` documents every theme key, and this gate is what makes that
//! sentence stay true instead of rotting the way per-hand checklists do.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use vigia::theme::Theme;

fn doc() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/THEME.md");
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("docs/THEME.md is unreadable at {}: {err}", path.display()))
}

/// The keys the document claims, read from its table rows only.
fn documented(source: &str) -> BTreeSet<String> {
    source
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("| `")?;
            let (key, _) = rest.split_once('`')?;
            Some(key.to_owned())
        })
        .collect()
}

#[test]
fn every_theme_key_has_a_row_in_the_reference() {
    let documented = documented(&doc());
    let missing: Vec<&str> = Theme::KEYS
        .iter()
        .copied()
        .filter(|key| !documented.contains(*key))
        .collect();
    assert!(
        missing.is_empty(),
        "keys with no row in docs/THEME.md: {missing:?}"
    );
}

#[test]
fn every_row_in_the_reference_is_a_key() {
    let stale: Vec<String> = documented(&doc())
        .into_iter()
        .filter(|key| !Theme::KEYS.contains(&key.as_str()))
        .collect();
    assert!(
        stale.is_empty(),
        "docs/THEME.md documents keys the palette does not have: {stale:?}"
    );
}

#[test]
fn the_reference_reads_keys_from_rows_at_all() {
    // The extractor above is the load-bearing part of both gates, and a change
    // to the document's table style would empty it, making both pass over a
    // document that documents nothing. Mutation-proofing per the house rule: a
    // check that cannot fail has not been written.
    assert!(
        documented(&doc()).len() >= Theme::KEYS.len(),
        "the table extractor found fewer rows than there are keys; \
         if the document's table style changed, change documented() with it"
    );
}
