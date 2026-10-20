//! The `syntect` fork parses as upstream does.

use std::process::Command;

#[test]
fn fork_parses_as_upstream() {
    let dump = include_bytes!("../assets/syntaxes.bin");
    let ours: syntect::parsing::SyntaxSet =
        syntect::dumps::from_uncompressed_data(dump).expect("the dump loads in the fork");
    let theirs: syntect_upstream::parsing::SyntaxSet =
        syntect_upstream::dumps::from_uncompressed_data(dump).expect("the dump loads upstream");
    let ours_md = ours
        .find_syntax_by_extension("md")
        .expect("a Markdown grammar");
    let theirs_md = theirs
        .find_syntax_by_extension("md")
        .expect("a Markdown grammar");

    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let listed = Command::new("git")
        .args(["ls-files", "*.md"])
        .current_dir(root)
        .output()
        .expect("git runs");
    let (mut files, mut fences) = (0, 0);
    for path in String::from_utf8_lossy(&listed.stdout).lines() {
        if path.starts_with("vendor/") {
            continue;
        }
        let text = std::fs::read_to_string(format!("{root}/{path}")).expect("read a tracked file");
        let mut mine = syntect::parsing::ParseState::new(ours_md);
        let mut upstream = syntect_upstream::parsing::ParseState::new(theirs_md);
        for (number, line) in text.split_inclusive('\n').enumerate() {
            fences += usize::from(line.trim_start().starts_with("```"));
            let a = mine.parse_line(line, &ours).expect("the fork parses");
            let b = upstream.parse_line(line, &theirs).expect("upstream parses");
            assert_eq!(
                format!("{a:?}"),
                format!("{b:?}"),
                "{path}:{} parses differently in the fork",
                number + 1
            );
        }
        files += 1;
    }
    assert!(
        files > 10 && fences > 10,
        "{files} files and {fences} fences, too few to prove anything"
    );
}
