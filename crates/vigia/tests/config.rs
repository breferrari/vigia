//! `SPEC.md` §11.2 B6 as amended: the pane a reader starts with.

use ratatui::layout::Rect;
use vigia::{Action, App, Config, ConfigError, Pointing, body_layout, config, diff_height};

/// A home directory holding a config file, or holding none.
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
    // The whole of what makes this amendment additive.
    let home = home_with("absent", None);
    let config = config::from_env(home_env(&home)).expect("no file is not an error");
    assert_eq!(config, Config::default());

    let plain = App::new();
    let configured = App::configured(config);
    assert_eq!(
        chrome_of(&configured),
        chrome_of(&plain),
        "a shell with no config file is not the shell `App::new` builds"
    );
}

/// What a config file can reach on the chrome, read off a drawn one.
fn chrome_of(app: &App) -> (bool, bool, bool, Option<usize>) {
    let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
    (chrome.masthead, chrome.rail, chrome.following, chrome.sheet)
}

#[test]
fn each_key_sets_the_state_the_pane_starts_in() {
    // One key at a time, so a parser that set the wrong field would be caught by the
    // two it should not have touched rather than only by the one it should.
    for (key, chrome) in [
        ("masthead", (true, false, true, None)),
        ("rail", (false, true, true, None)),
        // `single` is not on the chrome, so its row asserts the two that are stay
        // off: a mapping that sent it to `masthead` shows up there.
        ("single", (false, false, true, None)),
    ] {
        let home = home_with(&format!("app-{key}"), Some(&format!("{key} = on\n")));
        let config = config::from_env(home_env(&home)).expect("a config");
        assert_eq!(
            chrome_of(&App::configured(config)),
            chrome,
            "{key} = on reached the wrong field of the shell"
        );
    }

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
            single: true,
            staged: false,
            wrap: false,
            icons: false,
            // Untouched by the file, so the hand-written default holds: on.
            links: true,
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
    // A setting is a starting point rather than a decision, which is the sentence the
    // README makes and the one a reader would notice broken.
    let config = Config {
        masthead: true,
        rail: true,
        single: true,
        staged: false,
        wrap: false,
        icons: false,
        links: false,
    };
    let mut app = App::configured(config);

    let (masthead, rail, following, _) = chrome_of(&app);
    assert!(
        masthead && rail,
        "the configured shell did not start configured"
    );
    assert!(
        following,
        "a config file turned follow off, which is I5 and no key of this file"
    );

    let scratch = support::Scratch::large_diff("config-toggles", 6, 1);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    support::materialise(&mut frame);

    app.apply(Action::ToggleMasthead, &mut frame, 0)
        .expect("apply");
    app.apply(Action::ToggleRail, &mut frame, 0).expect("apply");
    let (masthead, rail, following, _) = chrome_of(&app);
    assert!(
        !masthead && !rail,
        "the keys did not toggle away from what the file asked for"
    );
    assert!(following, "toggling a view key disengaged follow");

    app.apply(Action::ToggleMasthead, &mut frame, 0)
        .expect("apply");
    let (masthead, _, _, _) = chrome_of(&app);
    assert!(masthead, "the key did not toggle back");
}

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

#[test]
fn a_key_this_file_does_not_have_names_its_line_and_refuses() {
    // Refused rather than ignored, which is the theme parser's rule for the
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
    // I5 as a gate rather than as a paragraph.
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

    // A trailing token is a typo, not a value with something after it, and the first
    // version of this parser took the first word and dropped the rest: `rail = on off`
    // set the rail and said nothing.
    for source in ["rail = on off\n", "rail=on=off\n", "single = on true\n"] {
        let err = config::parse(source).expect_err("a trailing token");
        assert!(
            matches!(&err, ConfigError::UnknownValue { value, .. } if value.contains(' ') || value.contains('=')),
            "{source:?} was accepted, or refused without quoting what it read: {err:?}"
        );
    }
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
    // A comment is not a value, which is the case a token-wise strip gets
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
    // Stricter than the theme file's ordinary keys, and the difference is `base`.
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
            single: true,
            staged: false,
            wrap: false,
            icons: false,
            // Untouched by the file, so the hand-written default holds: on.
            links: true,
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
    // The empty-versus-unset trap, which `theme::home_file` was written wrong once
    // already.
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

    // And `load` is where unreadable lives, which asserting through `from_env` gets
    // wrong: it filters on `is_file`, exactly as `theme::from_env` does, so a path that
    // exists and is not a file is *absent* rather than unreadable: a directory called
    // `config` is not a config file anybody wrote.
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
    // §11.2 B14 unchanged, reached from the file instead of from `r`.
    let app = App::configured(Config {
        rail: true,
        ..Config::default()
    });
    let chrome = app.chrome("fixture", None, Pointing::default(), 0, "");
    assert!(chrome.rail, "the file's request did not reach the chrome");

    let narrow = body_layout(Rect::new(0, 0, 100, 30), &chrome, 6, 6);
    assert!(
        !narrow.rail,
        "a hundred-column pane drew a rail, so the arrival width is not being read"
    );

    let wide = body_layout(Rect::new(0, 0, 160, 30), &chrome, 6, 6);
    assert!(
        wide.rail,
        "the request did not survive the narrow pane, so widening asks again"
    );
}

#[test]
fn the_configured_pane_is_the_pane_the_keys_would_have_made() {
    // The claim the whole amendment rests on, and the one no unit test of the parser
    // reaches: a file and three keystrokes have to arrive at the same shell.
    let scratch = support::Scratch::large_diff("config-equivalent", 6, 10);
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

    let mut configured = App::configured(Config {
        masthead: true,
        rail: true,
        single: true,
        staged: true,
        links: false,
        wrap: false,
        icons: false,
    });

    // Non-vacuity first, which every sibling has and this gate did not: two
    // identically broken shells agree with each other perfectly.
    assert_eq!(
        chrome_of(&configured),
        (true, true, true, None),
        "the configured shell is not configured, so the comparison below is \
         between two shells that both did nothing"
    );
    assert_eq!(
        chrome_of(&configured),
        chrome_of(&pressed),
        "the configured shell and the pressed shell are not the same shell"
    );

    // And `single`, which no comparison of chromes can reach.
    let body = diff_height(
        Rect::new(0, 0, 80, 24),
        &configured.chrome("fixture", None, Pointing::default(), 0, ""),
        6,
        6,
    );
    for app in [&mut configured, &mut pressed] {
        app.apply(Action::Bottom, &mut frame, body).expect("apply");
    }
    assert_eq!(
        configured.position(),
        pressed.position(),
        "the pin the file asked for and the pin `s` asks for send `G` to \
         different places"
    );
    assert_ne!(
        configured.position(),
        App::new().position(),
        "`G` under the pin landed where an untouched shell already was, so this \
         assertion cannot fail"
    );
}

#[test]
fn every_key_is_a_field_and_every_field_is_a_key() {
    // The tie between `KEYS` and `Config`'s fields, which the type system does not
    // give.
    let mut source = String::new();
    for key in vigia::config::KEYS {
        source.push_str(key);
        source.push_str(" = on\n");
    }
    assert_eq!(
        config::parse(&source).expect("every key in KEYS parses"),
        Config {
            masthead: true,
            rail: true,
            single: true,
            staged: true,
            links: true,
            wrap: true,
            icons: true,
        },
        "setting every key in KEYS did not set every field, so the two have drifted"
    );

    // And each one alone has to *change* something, which `is_ok` does not say.
    for key in vigia::config::KEYS {
        let lit = config::parse(&format!("{key} = on\n"))
            .unwrap_or_else(|why| panic!("KEYS names {key:?} and parse refuses it: {why}"));
        let unlit = config::parse(&format!("{key} = off\n"))
            .unwrap_or_else(|why| panic!("KEYS names {key:?} and parse refuses it: {why}"));
        assert_ne!(
            lit, unlit,
            "{key:?} is in KEYS and setting it changed nothing, so KEYS and \
             Config::set have drifted"
        );
    }
}

/// `staged = on` in the file reaches the frame, not just the shell.
#[test]
fn a_configured_staged_run_is_walked_on_the_first_frame() {
    let scratch = support::Scratch::new("config-staged");
    scratch.write("src/a.rs", "one\ntwo\n");
    scratch.write("src/b.rs", "alpha\n");
    scratch.git(&["add", "-A"]);
    scratch.git(&["commit", "-m", "init"]);
    scratch.write("src/a.rs", "one\nSTAGED\n");
    scratch.git(&["add", "src/a.rs"]);
    scratch.write("src/b.rs", "alpha\nUNSTAGED\n");

    let worktree = scratch.worktree();

    // What `main` does for a reader whose file says `staged = on`.
    let config = Config {
        staged: true,
        ..Config::default()
    };
    // Both halves of what `run` does for a reader whose file says `staged = on`:
    // the shell takes the config and so does the frame.
    let app = App::configured(config);
    assert!(app.staged(), "the shell did not take the setting");
    let mut frame = worktree.frame();
    vigia::arm_frame(&mut frame, config);
    frame.advance().expect("advance");

    assert!(
        frame
            .files()
            .iter()
            .any(|change| change.origin == vigia_core::Origin::Staged),
        "a shell configured with `staged = on` walked one comparison, so the key \
         sets a flag nothing acts on"
    );

    // And the default is untouched: a reader with no file gets one run.
    let plain = App::configured(Config::default());
    assert!(
        !plain.staged(),
        "a shell with no file took the setting anyway"
    );
    let mut frame = worktree.frame();
    vigia::arm_frame(&mut frame, Config::default());
    frame.advance().expect("advance");
    assert!(
        frame
            .files()
            .iter()
            .all(|change| change.origin == vigia_core::Origin::Unstaged),
        "a shell with no config file drew the staged run anyway"
    );
}
