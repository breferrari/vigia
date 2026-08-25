//! `SPEC.md` §11.2 **B6** as amended: the pane a reader starts with.
//!
//! Every gate here is one line of [#309](https://github.com/breferrari/vigia/issues/309)'s
//! own gate list, and the list is worth restating because it is what the ruling
//! promised rather than what the parser happens to do: no file is today's pane;
//! each key sets the state the pane starts in and the key still toggles from
//! there; a file that does not parse names its line and is refused **before** the
//! terminal is taken; absent is not an error and unreadable is; `rail = on` below
//! 134 columns keeps the request; and `follow` is not a key.
//!
//! **A separate binary from `palette.rs` for that file's own reason.** It gates the
//! *theme* file's discovery and grammar, and the two files share both, so a gate
//! living there would be answering the question through the wrong subject. What is
//! shared is the machinery; what is gated here is the ruling.
//!
//! The home directory is placed through the `lookup` both `from_env` functions
//! take, not through the process environment, so nothing here needs a lock and
//! nothing here can leak into another binary.

use ratatui::layout::Rect;
use vigia::{Action, App, Config, ConfigError, body_layout, config};

/// A home directory holding a config file, or holding none.
///
/// Named per test so two of them cannot collide in the temp directory, which is
/// `palette.rs`'s own shape one file over.
fn home_with(name: &str, contents: Option<&str>) -> std::path::PathBuf {
    let home = std::env::temp_dir().join(format!("vigia-config-{name}"));
    let dir = home.join(".config").join("vigia");
    std::fs::create_dir_all(&dir).expect("home");
    let file = dir.join("config");
    match contents {
        Some(text) => std::fs::write(&file, text).expect("write"),
        None => {
            let _ = std::fs::remove_file(&file);
        }
    }
    home
}

fn env_of(pairs: Vec<(String, String)>) -> impl Fn(&str) -> Option<String> {
    move |key| {
        pairs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.to_owned())
    }
}

fn home_env(home: &std::path::Path) -> impl Fn(&str) -> Option<String> {
    env_of(vec![("HOME".to_owned(), home.display().to_string())])
}

#[test]
fn no_file_is_not_an_error_and_is_todays_pane() {
    // **The whole of what makes this amendment additive.** A reader who has
    // written nothing sees what every version before it drew, so the file is a way
    // to say something rather than a requirement to say it. Asserted against
    // `App::new` rather than against three `false`s, because what has to hold is
    // that the two shells are the same shell.
    let home = home_with("absent", None);
    let config = config::from_env(home_env(&home)).expect("no file is not an error");
    assert_eq!(config, Config::default());

    let plain = App::new();
    let configured = App::configured(config);
    let area = Rect::new(0, 0, 200, 40);
    assert_eq!(
        chrome_of(&configured, area),
        chrome_of(&plain, area),
        "a shell with no config file is not the shell `App::new` builds"
    );
}

/// The three flags a config file can reach, read off a drawn chrome.
///
/// **Through `Chrome` rather than through `App`'s fields**, which are private on
/// purpose: what the ruling promises is a *pane*, and a gate reading the struct
/// would pass against a shell that stored the settings and drew none of them.
fn chrome_of(app: &App, _area: Rect) -> (bool, bool, Option<usize>) {
    let chrome = app.chrome("fixture", None, None, None, None, None);
    (chrome.masthead, chrome.rail, chrome.sheet)
}

#[test]
fn each_key_sets_the_state_the_pane_starts_in() {
    // One key at a time, so a parser that set the wrong field would be caught by
    // the two it should not have touched rather than only by the one it should.
    for (key, want) in [
        (
            "masthead",
            Config {
                masthead: true,
                ..Config::default()
            },
        ),
        (
            "rail",
            Config {
                rail: true,
                ..Config::default()
            },
        ),
        (
            "single",
            Config {
                single: true,
                ..Config::default()
            },
        ),
    ] {
        let home = home_with(&format!("one-{key}"), Some(&format!("{key} = on\n")));
        let got = config::from_env(home_env(&home)).expect("a config");
        assert_eq!(got, want, "{key} = on did not set {key} and only {key}");
    }

    // And all three together, which is the file a reader who wants the lot writes.
    let home = home_with("all", Some("masthead = on\nrail = on\nsingle = on\n"));
    assert_eq!(
        config::from_env(home_env(&home)).expect("a config"),
        Config {
            masthead: true,
            rail: true,
            single: true
        }
    );

    // `off` is not merely the default spelled out: it has to parse, or a reader
    // writing the state they are already in gets an error for saying nothing.
    let home = home_with("off", Some("single = off\nrail = off\n"));
    assert_eq!(
        config::from_env(home_env(&home)).expect("a config"),
        Config::default()
    );
}

#[test]
fn the_key_still_toggles_from_the_configured_state() {
    // **A setting is a starting point rather than a decision**, which is the
    // sentence the README makes and the one a reader would notice broken. A shell
    // that read the file and then refused the gesture would satisfy every other
    // gate here.
    let config = Config {
        masthead: true,
        rail: true,
        single: true,
    };
    let mut app = App::configured(config);
    let area = Rect::new(0, 0, 200, 40);

    let (masthead, rail, _) = chrome_of(&app, area);
    assert!(
        masthead && rail,
        "the configured shell did not start configured"
    );

    let scratch = support::Scratch::large_diff("config-toggles", 6, 1);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    support::materialise(&mut frame);

    app.apply(Action::ToggleMasthead, &mut frame, 0)
        .expect("apply");
    app.apply(Action::ToggleRail, &mut frame, 0).expect("apply");
    let (masthead, rail, _) = chrome_of(&app, area);
    assert!(
        !masthead && !rail,
        "the keys did not toggle away from what the file asked for"
    );

    app.apply(Action::ToggleMasthead, &mut frame, 0)
        .expect("apply");
    let (masthead, _, _) = chrome_of(&app, area);
    assert!(masthead, "the key did not toggle back");
}

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

#[test]
fn a_key_this_file_does_not_have_names_its_line_and_refuses() {
    // **Refused rather than ignored**, which is the theme parser's rule for the
    // theme parser's reason: a silently dropped key is a setting that does
    // nothing, and "it was discarded" is the one explanation a reader cannot
    // arrive at by looking at their screen.
    let err = config::parse("masthead = on\nsidebar = on\n").expect_err("an unknown key");
    assert_eq!(
        err,
        ConfigError::UnknownKey {
            line: 2,
            key: "sidebar".to_owned()
        }
    );
    // The line is the reader's, 1-based, and the message says which key.
    let said = err.to_string();
    assert!(
        said.contains("line 2"),
        "the error does not name the line: {said}"
    );
    assert!(
        said.contains("sidebar"),
        "the error does not name the key: {said}"
    );
}

#[test]
fn follow_is_not_a_key_this_file_accepts() {
    // **I5 as a gate rather than as a paragraph.** *Correct with zero interaction,
    // auto-follows the newest change* is a promise about the program, and a file
    // able to turn following off would quietly make it a promise about one
    // reader's configuration. It is the one plausible fourth key, which is why it
    // is refused by name here rather than left to the unknown-key gate above.
    let err = config::parse("follow = off\n").expect_err("follow is not a key");
    assert!(
        matches!(&err, ConfigError::UnknownKey { key, .. } if key == "follow"),
        "follow was accepted, or refused as something other than an unknown key: {err:?}"
    );
}

#[test]
fn a_value_that_is_neither_on_nor_off_names_its_line_and_its_key() {
    let err = config::parse("rail = yes\n").expect_err("an unknown value");
    assert_eq!(
        err,
        ConfigError::UnknownValue {
            line: 1,
            key: "rail".to_owned(),
            value: "yes".to_owned()
        }
    );
    let said = err.to_string();
    assert!(said.contains("rail") && said.contains("yes"), "{said}");
}

#[test]
fn a_missing_separator_and_a_missing_value_each_name_their_line() {
    assert_eq!(
        config::parse("masthead on\n").expect_err("no `=`"),
        ConfigError::MissingSeparator {
            line: 1,
            text: "masthead on".to_owned()
        }
    );
    assert_eq!(
        config::parse("rail =\n").expect_err("nothing after `=`"),
        ConfigError::MissingValue { line: 1 }
    );
    // **A comment is not a value**, which is the case a token-wise strip gets
    // right and a line-wise one does not: cutting at the first `#` would leave an
    // empty value that reports the same way, but accepting the `#` as the value
    // would report an unknown value naming a character rather than a missing one.
    assert_eq!(
        config::parse("rail = # oops\n").expect_err("only a comment after `=`"),
        ConfigError::MissingValue { line: 1 }
    );
}

#[test]
fn the_same_key_twice_is_refused_rather_than_last_wins() {
    // **Stricter than the theme file's ordinary keys, and the difference is
    // `base`.** A theme is a base plus overrides, so a later line legitimately
    // replaces an earlier one. This file has no base, so no line can be an
    // intentional override of another and every repeat is a mistake, which is the
    // reasoning `ThemeError::RepeatedBase` already applies to the one theme key
    // with nothing above it.
    let err =
        config::parse("single = on\nrail = on\nsingle = off\n").expect_err("the same key twice");
    assert_eq!(
        err,
        ConfigError::RepeatedKey {
            line: 3,
            key: "single".to_owned(),
            first: 1
        }
    );
    let said = err.to_string();
    assert!(
        said.contains("line 3") && said.contains("line 1"),
        "the error names one line but not the other: {said}"
    );
}

#[test]
fn comments_and_blank_lines_and_a_byte_order_mark_are_all_survivable() {
    // The three the theme parser's own header calls out, gated here because the
    // grammar is shared and a copy that dropped one of them would be a file that
    // works everywhere except on the machine that writes a BOM.
    let source = "\u{FEFF}# the pane I want\n\n  masthead = on   # the churn band\n\nsingle = on\n";
    assert_eq!(
        config::parse(source).expect("a config"),
        Config {
            masthead: true,
            rail: false,
            single: true
        }
    );

    // And the BOM specifically: U+FEFF is `Cf` rather than `White_Space`, so it
    // survives every trim and lands inside the first key.
    assert_eq!(
        config::parse("\u{FEFF}rail = on\n").expect("a config"),
        Config {
            rail: true,
            ..Config::default()
        }
    );
}

#[test]
fn an_empty_home_falls_through_rather_than_being_taken_as_one() {
    // **The empty-versus-unset trap, which `theme::home_file` was written wrong
    // once already.** An environment variable has three states and the third only
    // shows up on somebody else's machine: `HOME=""` is `Some("")`, so a fallback
    // written as `or_else` never fires and `USERPROFILE` is skipped having already
    // been discarded.
    let home = home_with("empty-home", Some("single = on\n"));
    let lookup = env_of(vec![
        ("HOME".to_owned(), "   ".to_owned()),
        ("USERPROFILE".to_owned(), home.display().to_string()),
    ]);
    assert_eq!(
        config::from_env(lookup).expect("a config"),
        Config {
            single: true,
            ..Config::default()
        },
        "a blank HOME was taken as a home, so USERPROFILE was never tried"
    );

    // And no home at all is no file, which is not an error.
    assert_eq!(
        config::from_env(env_of(vec![])).expect("no home is not an error"),
        Config::default()
    );
}

#[test]
fn absent_is_not_an_error_and_unreadable_is() {
    // The distinction the theme file draws, for its reason: nobody has to have a
    // file, and a reader who wrote one and got the defaults silently would have no
    // way to find out why.
    let home = home_with("unreadable", None);
    assert_eq!(
        config::from_env(home_env(&home)).expect("absent is not an error"),
        Config::default()
    );

    // **And `load` is where unreadable lives, which is worth stating because the
    // first draft of this gate asserted it through `from_env` and was wrong.**
    // `from_env` filters on `is_file`, exactly as `theme::from_env` does, so a
    // path that exists and is not a file is *absent* rather than unreadable: a
    // directory called `config` is not a config file anybody wrote. What reports
    // `Unreadable` is a path that is a file and will not read, and `load` is the
    // function every such path goes through.
    let dir = home.join(".config").join("vigia").join("as-a-directory");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory to read as a file");
    let err = config::load(&dir).expect_err("a directory does not read as a file");
    assert!(
        matches!(err, ConfigError::Unreadable { .. }),
        "reading a directory reported {err:?}"
    );
    assert!(
        err.to_string().contains("as-a-directory"),
        "the error does not name the path: {err}"
    );
    let _ = std::fs::remove_dir_all(&dir);

    // The filter itself, asserted rather than assumed: a non-file where the config
    // belongs falls through to the defaults instead of failing the shell.
    let odd = home_with("not-a-file", None);
    let at = odd.join(".config").join("vigia").join("config");
    let _ = std::fs::remove_file(&at);
    std::fs::create_dir_all(&at).expect("a directory in the file's place");
    assert_eq!(
        config::from_env(home_env(&odd)).expect("a non-file is absent"),
        Config::default(),
        "a directory where the file goes failed the shell instead of being absent"
    );
    let _ = std::fs::remove_dir_all(&at);
}

#[test]
fn a_railed_default_below_the_arrival_width_keeps_the_request() {
    // **§11.2 B14 unchanged, reached from the file instead of from `r`.** A pane
    // under 134 columns draws no rail whatever the request says, and the request
    // is kept rather than cleared, so widening produces the rail rather than the
    // question. What the file sets is `Chrome::rail`; what the pane can give is
    // `Body::rail`.
    let app = App::configured(Config {
        rail: true,
        ..Config::default()
    });
    let chrome = app.chrome("fixture", None, None, None, None, None);
    assert!(chrome.rail, "the file's request did not reach the chrome");

    let narrow = body_layout(Rect::new(0, 0, 100, 30), &chrome, 6);
    assert!(
        !narrow.rail,
        "a hundred-column pane drew a rail, so the arrival width is not being read"
    );

    let wide = body_layout(Rect::new(0, 0, 160, 30), &chrome, 6);
    assert!(
        wide.rail,
        "the request did not survive the narrow pane, so widening asks again"
    );
}

#[test]
fn the_configured_pane_is_the_pane_the_keys_would_have_made() {
    // **The claim the whole amendment rests on**, and the one no unit test of the
    // parser reaches: a file and three keystrokes have to arrive at the same
    // shell. If they can differ, the file is a second implementation of the
    // toggles rather than a default for them.
    let scratch = support::Scratch::large_diff("config-equivalent", 6, 1);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    support::materialise(&mut frame);

    let mut pressed = App::new();
    for action in [
        Action::ToggleMasthead,
        Action::ToggleRail,
        Action::ToggleSingle,
    ] {
        pressed.apply(action, &mut frame, 0).expect("apply");
    }

    let configured = App::configured(Config {
        masthead: true,
        rail: true,
        single: true,
    });

    let area = Rect::new(0, 0, 200, 40);
    assert_eq!(
        chrome_of(&configured, area),
        chrome_of(&pressed, area),
        "the configured shell and the pressed shell are not the same shell"
    );

    // And the layouts they produce agree, which is the half a chrome comparison
    // cannot see: `single` reaches the walk rather than the chrome.
    let a = body_layout(
        area,
        &configured.chrome("fixture", None, None, None, None, None),
        6,
    );
    let b = body_layout(
        area,
        &pressed.chrome("fixture", None, None, None, None, None),
        6,
    );
    assert_eq!((a.diff, a.list, a.rail), (b.diff, b.list, b.rail));
}
