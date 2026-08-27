//! The shell's half of the warm policy: when a demand is worth spawning for.

use vigia::worth_warming;

/// Nothing on screen is waiting for colour, so nothing is spawned.
#[test]
fn an_empty_demand_is_not_worth_a_thread() {
    assert!(!worth_warming(&[], &[], false));
    assert!(!worth_warming(&[], &["src/a.rs".to_owned()], false));
}

/// A demand nobody has been handed yet is spawned for.
#[test]
fn a_fresh_demand_is_worth_a_thread() {
    assert!(worth_warming(&["src/a.rs".to_owned()], &[], false));
    assert!(worth_warming(
        &["src/a.rs".to_owned()],
        &["src/b.rs".to_owned()],
        false
    ));
}

/// **The defect this rule exists for.** A demand the last warm was already
/// handed, and which came back unchanged, is not offered again.
#[test]
fn a_demand_that_did_not_move_is_not_offered_twice() {
    let demand = vec!["ui/strings.ts".to_owned()];
    assert!(worth_warming(&demand, &[], false));
    assert!(
        !worth_warming(&demand, &demand, false),
        "the same demand was offered to a second warm, so a file the warmer \
         cannot serve costs a thread and a wake on every frame it is on screen"
    );
}

/// And it cannot stall a demand that **is** making progress.
#[test]
fn a_demand_that_shrank_is_offered_again_immediately() {
    let before = vec![
        "src/a.rs".to_owned(),
        "docs/b.md".to_owned(),
        "ui/c.ts".to_owned(),
    ];
    let after = vec!["ui/c.ts".to_owned()];

    assert!(
        worth_warming(&after, &before, false),
        "a demand that lost two of its three grammars was treated as the same \
         demand, so a warm that made partial progress stops the rest arriving"
    );
}

/// Order is part of the identity, because the frame's order is the order the
/// warmer spends its budget in.
#[test]
fn a_reordered_demand_is_a_different_demand() {
    let before = vec!["src/a.rs".to_owned(), "docs/b.md".to_owned()];
    let after = vec!["docs/b.md".to_owned(), "src/a.rs".to_owned()];

    assert!(worth_warming(&after, &before, false));
}

/// **A tick overrides the memo, and exactly once.**
#[test]
fn a_write_reopens_a_demand_the_memo_is_holding_back() {
    let demand = vec!["ui/strings.ts".to_owned()];

    assert!(
        !worth_warming(&demand, &demand, false),
        "the memo is not holding this back, so the case below proves nothing"
    );
    assert!(
        worth_warming(&demand, &demand, true),
        "a write landed and the demand was still held back, so a file the \
         warmer could not open once stays plain for the session"
    );
}

/// And a write does not make an **empty** demand worth a thread.
#[test]
fn a_write_does_not_conjure_a_demand_from_nothing() {
    assert!(!worth_warming(&[], &[], true));
    assert!(!worth_warming(&[], &["src/a.rs".to_owned()], true));
}
