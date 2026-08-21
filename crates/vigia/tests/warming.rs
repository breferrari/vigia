//! The shell's half of the warm policy: when a demand is worth spawning for.
//!
//! `vigia_core` decides *which* grammars a frame is waiting on
//! (`Highlighter::wanted`). This file covers the other half, which is the
//! shell's, and which exists because of I1: every warm ends by sending
//! `Wake::Warmed`, so a demand that is offered forever is a **thread and a wake
//! per frame** on a tree nobody is touching, and I1's budget is *0 wakeups while
//! idle*.
//!
//! The core stops the ordinary case on its own: `warm_run` marks the grammar it
//! had a run at, even for a file that vanished before it could be opened, so
//! such a grammar is parsed cold once rather than asked for forever. What it
//! cannot do is mark the **right** grammar for a file it never opened whose
//! extension lies about its language, which is `SPEC.md` §6's Qt `.ts` rule: the
//! frame draws it as XML because the frame has its first line, and a warmer that
//! never opened it can only mark TypeScript.
//!
//! Found by the [#129](https://github.com/breferrari/vigia/issues/129) audit
//! rather than by a gate, which is why the rule was lifted out of the loop into
//! a function that can have one.

use vigia::worth_warming;

/// Nothing on screen is waiting for colour, so nothing is spawned.
///
/// The steady state a reader sits in for hours, and the one where I1's budget is
/// measured.
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
///
/// Without this, a Qt `.ts` file deleted before the warmer opened it is demanded
/// on every frame for the rest of the session: the warmer marks TypeScript, the
/// frame is waiting on XML, and neither ever learns better. Every one of those
/// demands spawns a thread and sends a wake.
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
///
/// The half that keeps the rule from being a colour bug: a warm that compiles
/// two of three grammars leaves a shorter list, which is a different list, which
/// is offered at once. Only a demand that moved not at all is held back.
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
///
/// `Highlighter::wanted` lists paths in the order the frame reached them, and
/// `warm_run` takes them in that order under `WARM_TOTAL`. Two lists holding the
/// same paths in a different order therefore spend the budget differently, so
/// they are different demands and the second one is worth offering.
#[test]
fn a_reordered_demand_is_a_different_demand() {
    let before = vec!["src/a.rs".to_owned(), "docs/b.md".to_owned()];
    let after = vec!["docs/b.md".to_owned(), "src/a.rs".to_owned()];

    assert!(worth_warming(&after, &before, false));
}

/// **A tick overrides the memo, and exactly once.**
///
/// A tick is the world changing, so a demand that could not be served a moment
/// ago is worth offering again: a file that had vanished may be back, and the
/// `SPEC.md` §6 Qt case that motivates the memo is precisely a file the warmer
/// could not open. Without this, one unservable demand keeps its hunk plain for
/// the rest of the session however many times it is rewritten.
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
///
/// The one place the override must not reach: with nothing on screen waiting
/// for colour there is nothing to warm, and a tick is the most common wake this
/// program has. Spawning a thread per tick on a settled screen is the busy loop
/// the whole memo exists to prevent, arriving from the other side.
#[test]
fn a_write_does_not_conjure_a_demand_from_nothing() {
    assert!(!worth_warming(&[], &[], true));
    assert!(!worth_warming(&[], &["src/a.rs".to_owned()], true));
}
