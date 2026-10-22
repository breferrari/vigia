//! The `syntect` fork parses as upstream does.

use std::process::Command;

#[test]
fn fork_parses_as_upstream() {
    let dump = include_bytes!("../assets/syntaxes.bin");
    let ours: syntect::parsing::SyntaxSet =
        syntect::dumps::from_uncompressed_data(dump).expect("the dump loads in the fork");
    let theirs: syntect_upstream::parsing::SyntaxSet =
        syntect_upstream::dumps::from_uncompressed_data(dump).expect("the dump loads upstream");

    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let listed = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root)
        .output()
        .expect("git runs");
    let (mut files, mut grammars, mut lines) = (0, std::collections::BTreeSet::new(), 0);
    for path in String::from_utf8_lossy(&listed.stdout).split('\0') {
        // The vendored crates are more Rust, read by a grammar this already covers.
        if path.is_empty() || path.starts_with("vendor/") {
            continue;
        }
        let full = format!("{root}/{path}");
        let (Ok(Some(mine)), Ok(Some(upstream))) = (
            ours.find_syntax_for_file(&full),
            theirs.find_syntax_for_file(&full),
        ) else {
            continue;
        };
        assert_eq!(
            mine.name, upstream.name,
            "{path} picks another grammar in the fork"
        );
        let text = std::fs::read_to_string(&full)
            .unwrap_or_else(|error| panic!("{path} has a grammar and cannot be read: {error}"));
        let mut a = syntect::parsing::ParseState::new(mine);
        let mut b = syntect_upstream::parsing::ParseState::new(upstream);
        for (number, line) in text.split_inclusive('\n').enumerate() {
            let ours_ops = a.parse_line(line, &ours).expect("the fork parses");
            let theirs_ops = b.parse_line(line, &theirs).expect("upstream parses");
            assert_eq!(
                format!("{ours_ops:?}"),
                format!("{theirs_ops:?}"),
                "{path}:{} parses differently in the fork",
                number + 1
            );
            lines += 1;
        }
        files += 1;
        grammars.insert(mine.name.clone());
    }
    assert!(
        files > 50 && grammars.len() > 5 && grammars.contains("Markdown"),
        "{files} files in {grammars:?}, too few to prove anything"
    );
    eprintln!("{files} files, {lines} lines, grammars {grammars:?}");
}
