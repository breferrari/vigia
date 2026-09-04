//! The startup update check: what the environment can decline, what the answer
//! is read as, and that asking costs the first paint nothing.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use vigia::update::{UPDATE_VAR, newer, version_in, wanted, watch};
use vigia::{App, Glyphs, Pointing, Theme, View, render};

/// An environment built from pairs, so a case reads as the thing it is testing.
fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
    let owned: Vec<(String, String)> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    move |key| {
        owned
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.to_owned())
    }
}

/// What the endpoint answered on 2026-09-04, trimmed to the fields read.
const ANSWER: &str = r#"{"crate":{"id":"vigia","name":"vigia",
  "description":"A live diff monitor for the terminal.",
  "max_version":"0.34.0","newest_version":"0.34.0","max_stable_version":"0.34.0",
  "downloads":782,"repository":"https://github.com/breferrari/vigia"}}"#;

#[test]
fn an_unset_variable_asks_for_the_check() {
    assert_eq!(wanted(env(&[])), Ok(true));
}

#[test]
fn off_declines_it() {
    assert_eq!(wanted(env(&[(UPDATE_VAR, "off")])), Ok(false));
    assert_eq!(wanted(env(&[(UPDATE_VAR, "  OFF  ")])), Ok(false));
}

#[test]
fn a_value_it_does_not_understand_is_refused() {
    let refused =
        wanted(env(&[(UPDATE_VAR, "no")])).expect_err("a value nobody defined was accepted");
    assert_eq!(refused.value, "no");
    assert!(
        refused.to_string().contains("auto, off"),
        "the refusal does not say what would have been accepted: {refused}"
    );
}

#[test]
fn a_variable_set_to_nothing_is_not_a_value() {
    // A PowerShell `$env:VIGIA_UPDATE = ""` is set and empty, which is the
    // shape the colour ladder already refuses to read as an override.
    assert_eq!(wanted(env(&[(UPDATE_VAR, "")])), Ok(true));
    assert_eq!(wanted(env(&[(UPDATE_VAR, "   ")])), Ok(true));
}

#[test]
fn a_newer_release_is_newer() {
    assert!(newer("0.34.1", "0.34.0"));
    assert!(newer("0.35.0", "0.34.9"));
}

#[test]
fn the_same_release_is_not() {
    assert!(!newer("0.34.0", "0.34.0"));
}

#[test]
fn an_older_release_is_not() {
    assert!(!newer("0.33.9", "0.34.0"));
    assert!(!newer("0.34.0", "1.0.0"));
}

#[test]
fn a_major_bump_is_newer() {
    assert!(newer("1.0.0", "0.99.99"));
}

#[test]
fn a_version_that_is_not_a_triple_says_nothing() {
    for odd in ["0.35", "0.35.0.1", "0.35.0-beta.1", "v0.35.0", "", "latest"] {
        assert!(
            !newer(odd, "0.34.0"),
            "{odd:?} was read as a release when it is not one, so the footer \
             would name a version that does not exist"
        );
    }
}

#[test]
fn the_body_the_api_returns_carries_the_stable_version() {
    assert_eq!(version_in(ANSWER).as_deref(), Some("0.34.0"));
}

#[test]
fn a_prerelease_is_not_what_gets_named() {
    let body = r#"{"crate":{"newest_version":"0.35.0-rc.1","max_stable_version":"0.34.0"}}"#;
    assert_eq!(version_in(body).as_deref(), Some("0.34.0"));
}

#[test]
fn a_body_that_is_not_json_says_nothing() {
    for body in ["", "<html>502 Bad Gateway</html>", "{\"crate\":"] {
        assert_eq!(version_in(body), None, "{body:?} parsed as an answer");
    }
}

#[test]
fn a_body_without_the_field_says_nothing() {
    assert_eq!(version_in(r#"{"errors":[{"detail":"Not Found"}]}"#), None);
    assert_eq!(version_in(r#"{"crate":{"name":"vigia"}}"#), None);
}

/// I7: the request is 612ms measured and the first paint has 50ms, so the check
/// has to hand the caller back before it starts rather than after.
#[test]
fn the_check_never_delays_the_caller() {
    let slow = Duration::from_millis(500);
    let (tx, rx) = mpsc::channel();

    let began = Instant::now();
    watch(
        move || {
            std::thread::sleep(slow);
            Some("9.9.9".to_owned())
        },
        move |version| {
            let _ = tx.send(version);
        },
    );
    let handed_back = began.elapsed();

    // The answer first, so what follows cannot be satisfied by a check that
    // never ran: an absence is free until the work is counted.
    let answered = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the check never answered, so this proves nothing about its cost");
    assert_eq!(answered, "9.9.9");
    assert!(
        began.elapsed() >= slow,
        "the check answered before it could have done the work"
    );
    assert!(
        handed_back < slow / 4,
        "the caller waited {handed_back:?} of a {slow:?} check, so a slow \
         registry would be a slow first paint"
    );
}

#[test]
fn a_check_that_answers_nothing_says_nothing() {
    let (tx, rx) = mpsc::channel();
    watch(
        || None,
        move |version| {
            let _ = tx.send(version);
        },
    );
    // Disconnected rather than a timeout, and that is the stronger reading: the
    // sender only drops once the thread has finished, so this says the check ran
    // and stayed quiet rather than that it is merely slow.
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(10)),
        Err(mpsc::RecvTimeoutError::Disconnected),
        "a check with no answer still told the footer something"
    );
}

/// The narrow rung, because a notice is one token that takes what the line has.
#[test]
fn the_footer_carries_the_version_it_was_told() {
    for width in [40, 80] {
        let mut app = App::new();
        app.flash("vigia 9.9.9 is available");
        let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");

        let area = Rect::new(0, 0, width, 24);
        let mut buf = Buffer::empty(area);
        render(
            &mut buf,
            area,
            &View::default(),
            &Theme::default(),
            Glyphs::default(),
            &chrome,
        );

        let drawn: String = (0..area.height)
            .map(|y| (0..area.width).map(|x| buf[(x, y)].symbol()).collect())
            .collect::<Vec<String>>()
            .join("\n");
        assert!(
            drawn.contains("9.9.9"),
            "a {width}-column pane never drew the version:\n{drawn}"
        );
    }
}

/// A guard that can never be true would turn the check off everywhere and
/// nothing else would notice, because silence is also what a failure looks like.
#[test]
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn the_machine_running_this_can_run_the_provider() {
    assert!(
        vigia::update::provider_runs_here(),
        "no machine this suite runs on is below the provider's floor, so a \
         false here is the guard naming something rather than the CPU lacking it"
    );
}

/// The wiring `run` holds, which no test can enter and every gate above would
/// stay green without.
///
/// A text scan is the only layer available here, so it is deliberately narrow:
/// it says the three seams exist, not that they are correct. What it stops is
/// the whole feature being deleted from the shell while its own suite passes.
///
/// Comment lines are dropped first, because commenting the block out is the
/// cheapest way to delete it and a scan that reads them cannot tell the two
/// apart. Found by doing exactly that and watching this pass.
#[test]
fn the_shell_still_arms_the_check() {
    let code: String = include_str!("../src/lib.rs")
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join(
            "
",
        );
    assert!(
        code.len() > 1000 && code.contains("pub fn run("),
        "the shell was not read, so scanning it proves nothing"
    );
    for seam in ["update::wanted(", "update::watch(", "Wake::Update(version)"] {
        assert!(
            code.contains(seam),
            "`{seam}` is gone from the shell, so nothing asks and nothing draws"
        );
    }
}

/// The half CI cannot hold: a real request, against the real registry.
///
/// Every other test here stops at the seam, because a gate that needs a network
/// is a gate that fails on a train. Run it by hand after touching `fetch`, which
/// is the only code in this module nothing else covers.
#[test]
#[ignore = "an instrument: it asks the real registry"]
fn the_registry_answers_a_version_this_can_read() {
    let began = Instant::now();
    let answer = vigia::update::check("0.0.1");
    println!("check(0.0.1) -> {answer:?} in {:?}", began.elapsed());
    assert!(
        answer.is_some(),
        "no version came back, so either the registry moved or the answer stopped parsing"
    );
    assert_eq!(
        vigia::update::check("999.0.0"),
        None,
        "a version above every published one was still called an update"
    );
}
