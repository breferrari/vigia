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
    let configured = App::configured(&config);
    assert_eq!(
        chrome_of(&configured),
        chrome_of(&plain),
        "a shell with no config file is not the shell `App::new` builds"
    );
}

/// What a config file can reach on the chrome, read off a drawn one.
fn chrome_of(app: &App) -> (bool, bool, bool, Option<usize>) {
    let chrome = app.chrome(
        "fixture",
        None,
        "current",
        Pointing::default(),
        Default::default(),
        "",
    );
    (chrome.rail, chrome.overview, chrome.following, chrome.sheet)
}

#[test]
fn each_key_sets_the_state_the_pane_starts_in() {
    // One key at a time, so a parser that set the wrong field would be caught by
    // the one it should not have touched rather than only by the one it should.
    for (key, chrome) in [
        ("rail", (true, false, true, None)),
        ("overview", (false, true, true, None)),
        // `single` is not on the chrome, so its row asserts the ones that are stay
        // off: a mapping that sent it to `rail` shows up there.
        ("single", (false, false, true, None)),
    ] {
        let home = home_with(&format!("app-{key}"), Some(&format!("{key} = on\n")));
        let config = config::from_env(home_env(&home)).expect("a config");
        assert_eq!(
            chrome_of(&App::configured(&config)),
            chrome,
            "{key} = on reached the wrong field of the shell"
        );
    }

    for (key, want) in [
        (
            "rail",
            Config {
                follow: true,
                persist: false,
                rail: true,
                ..Config::default()
            },
        ),
        (
            "single",
            Config {
                follow: true,
                persist: false,
                single: true,
                ..Config::default()
            },
        ),
        (
            "overview",
            Config {
                follow: true,
                persist: false,
                overview: true,
                ..Config::default()
            },
        ),
    ] {
        let home = home_with(&format!("one-{key}"), Some(&format!("{key} = on\n")));
        let got = config::from_env(home_env(&home)).expect("a config");
        assert_eq!(got, want, "{key} = on did not set {key} and only {key}");
    }

    // And both together, which is the file a reader who wants the lot writes.
    let home = home_with("all", Some("rail = on\nsingle = on\n"));
    assert_eq!(
        config::from_env(home_env(&home)).expect("a config"),
        Config {
            follow: true,
            persist: false,
            rail: true,
            single: true,
            overview: false,
            staged: false,
            wrap: false,
            icons: false,
            // Untouched by the file, so the hand-written defaults hold: on.
            notes: true,
            links: true,
            hide: None,
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
        follow: true,
        persist: false,
        rail: true,
        single: true,
        overview: false,
        staged: false,
        wrap: false,
        notes: false,
        icons: false,
        links: false,
        hide: None,
    };
    let mut app = App::configured(&config);

    let (rail, _, following, _) = chrome_of(&app);
    assert!(rail, "the configured shell did not start configured");
    assert!(
        following,
        "a config file with `follow` on did not start the pane following"
    );

    let scratch = support::Scratch::large_diff("config-toggles", 6, 1);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    support::materialise(&mut frame);

    app.apply(Action::ToggleRail, &mut frame, 0).expect("apply");
    let (rail, _, following, _) = chrome_of(&app);
    assert!(
        !rail,
        "the key did not toggle away from what the file asked for"
    );
    assert!(following, "toggling a view key disengaged follow");

    app.apply(Action::ToggleRail, &mut frame, 0).expect("apply");
    let (rail, _, _, _) = chrome_of(&app);
    assert!(rail, "the key did not toggle back");
}

#[path = "../../vigia-core/tests/support/mod.rs"]
mod support;

#[path = "support/mod.rs"]
mod screen;

use screen::{Place, actions_keys_reach, place_of};

#[test]
fn a_key_this_file_does_not_have_names_its_line_and_refuses() {
    // Refused rather than ignored, which is the theme parser's rule for the
    // theme parser's reason: a silently dropped key is a setting that does
    // nothing, and "it was discarded" is the one explanation a reader cannot
    // arrive at by looking at their screen.
    let err = config::parse("rail = on\nsidebar = on\n").expect_err("an unknown key");
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
        config::parse("single on\n").expect_err("no `=`"),
        ConfigError::MissingSeparator {
            line: 1,
            text: "single on".to_owned()
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
    let source =
        "\u{FEFF}# the pane I want\n\n  rail = on   # the list beside the diff\n\nsingle = on\n";
    assert_eq!(
        config::parse(source).expect("a config"),
        Config {
            follow: true,
            persist: false,
            rail: true,
            single: true,
            overview: false,
            staged: false,
            wrap: false,
            icons: false,
            // Untouched by the file, so the hand-written defaults hold: on.
            notes: true,
            links: true,
            hide: None,
        }
    );

    // And the BOM specifically: U+FEFF is `Cf` rather than `White_Space`, so it
    // survives every trim and lands inside the first key.
    assert_eq!(
        config::parse("\u{FEFF}rail = on\n").expect("a config"),
        Config {
            follow: true,
            persist: false,
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
            follow: true,
            persist: false,
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
    let app = App::configured(&Config {
        follow: true,
        persist: false,
        rail: true,
        ..Config::default()
    });
    let chrome = app.chrome(
        "fixture",
        None,
        "current",
        Pointing::default(),
        Default::default(),
        "",
    );
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
    // reaches: a file and two keystrokes have to arrive at the same shell.
    let scratch = support::Scratch::large_diff("config-equivalent", 6, 10);
    let worktree = scratch.worktree();
    let mut frame = worktree.frame();
    support::materialise(&mut frame);

    let mut pressed = App::new();
    for action in [Action::ToggleRail, Action::ToggleSingle] {
        pressed.apply(action, &mut frame, 0).expect("apply");
    }

    let mut configured = App::configured(&Config {
        follow: true,
        persist: false,
        rail: true,
        single: true,
        overview: false,
        staged: true,
        links: false,
        wrap: false,
        notes: false,
        icons: false,
        hide: None,
    });

    // Non-vacuity first, which every sibling has and this gate did not: two
    // identically broken shells agree with each other perfectly.
    assert_eq!(
        chrome_of(&configured),
        (true, false, true, None),
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
        &configured.chrome(
            "fixture",
            None,
            "current",
            Pointing::default(),
            Default::default(),
            "",
        ),
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
            follow: true,
            persist: true,
            rail: true,
            single: true,
            overview: true,
            staged: true,
            links: true,
            wrap: true,
            notes: true,
            icons: true,
            hide: None,
        },
        "setting every key in KEYS did not set every field, so the two have drifted"
    );

    // And each one alone has to *change* something, which `is_ok` does not say.
    let mut apart: Vec<(&str, Config, Config)> = Vec::new();
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
        apart.push((key, lit, unlit));
    }

    // And no two keys may move the same field. The assertions above cannot see
    // that: a key wired to a neighbour's field leaves its own at the default,
    // and where that default is already `on` the whole-file comparison holds
    // anyway, while `lit != unlit` holds because the neighbour moved. What
    // separates them is the pair, since a key that starts `on` and one that
    // starts `off` agree on one side of it and never on both.
    for (at, (key, lit, unlit)) in apart.iter().enumerate() {
        for (other, other_lit, other_unlit) in &apart[at + 1..] {
            assert!(
                (lit, unlit) != (other_lit, other_unlit),
                "{key:?} and {other:?} move the same field, so one of them is \
                 wired to the other's and its own is whatever the default left there"
            );
        }
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
        follow: true,
        persist: false,
        staged: true,
        ..Config::default()
    };
    // Both halves of what `run` does for a reader whose file says `staged = on`:
    // the shell takes the config and so does the frame.
    let app = App::configured(&config);
    assert!(app.staged(), "the shell did not take the setting");
    let mut frame = worktree.frame();
    vigia::arm_frame(&mut frame, &config);
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
    let plain = App::configured(&Config::default());
    assert!(
        !plain.staged(),
        "a shell with no file took the setting anyway"
    );
    let mut frame = worktree.frame();
    vigia::arm_frame(&mut frame, &Config::default());
    frame.advance().expect("advance");
    assert!(
        frame
            .files()
            .iter()
            .all(|change| change.origin == vigia_core::Origin::Unstaged),
        "a shell with no config file drew the staged run anyway"
    );
}

#[test]
fn every_view_toggle_has_a_key_or_a_reason() {
    let (mut keyed, mut excluded) = (0usize, 0usize);
    for action in actions_keys_reach() {
        match place_of(&action) {
            Place::Key { key, .. } => {
                assert!(
                    config::KEYS.contains(&key),
                    "{action:?} is set by {key:?} and the file accepts no such key, \
                     so a reader cannot start the pane where the gesture puts it"
                );
                keyed += 1;
            }
            Place::Excluded(why) => {
                assert!(
                    !why.trim().is_empty(),
                    "{action:?} is excluded and says no reason, which is the \
                     oversight this gate exists to tell from a ruling"
                );
                excluded += 1;
            }
            Place::Neither(why) => assert!(
                !why.trim().is_empty(),
                "{action:?} is called no toggle and says no reason, so the \
                 classification cannot be argued with"
            ),
        }
    }

    // Non-vacuity: a sweep that reached no toggle at all would pass every
    // assertion above by walking nothing.
    assert!(
        keyed > 0 && excluded > 0,
        "the sweep found {keyed} keyed toggle(s) and {excluded} excluded, so it is \
         not walking the keymap and the assertions above are over an empty set"
    );

    // `b` is the whole exclusion list since B22: `f` left it when the menu made
    // what a reader flips worth keeping, and `REVOCATIONS.md` holds what its
    // exclusion said.
    assert!(
        matches!(place_of(&Action::ToggleStanding), Place::Excluded(_)),
        "`b` left the exclusion list, and `SPEC.md` §11.2 B6 is what has to change \
         before this does"
    );
    assert!(
        matches!(
            place_of(&Action::ToggleFollow),
            Place::Key { key: "follow", .. }
        ),
        "`f` is not a key of the file, which B22 ruled it is"
    );
}

#[test]
fn every_key_the_file_accepts_is_a_gesture_or_is_config_only() {
    // The three `SPEC.md` §11.2 B6 names as reaching no key at all, which is why
    // they cannot come out of the sweep. `persist` joined them with B22: it is a row
    // of the config menu and no key of its own.
    const CONFIG_ONLY: [&str; 3] = ["icons", "links", "persist"];

    let mut reachable: Vec<&str> = actions_keys_reach()
        .iter()
        .filter_map(|action| match place_of(action) {
            Place::Key { key, .. } => Some(key),
            Place::Excluded(_) | Place::Neither(_) => None,
        })
        .collect();
    reachable.extend(CONFIG_ONLY);
    reachable.sort_unstable();

    // The valued settings are not in this comparison, because the sweep is over
    // the keymap and a valued setting is reachable by no gesture at all. That is
    // the difference the two lists exist to hold, rather than a second exclusion
    // list with a reason on it.
    let mut accepted: Vec<&str> = config::KEYS.to_vec();
    accepted.sort_unstable();

    assert_eq!(
        reachable, accepted,
        "a key here and not there is a gesture the file cannot start; a key there \
         and not here is one no gesture and no ruling accounts for"
    );

    // And the parser accepts exactly the union of the two lists: a key on one of
    // them the parser refuses is a list that has drifted, and a key on neither
    // that the parser takes is a setting no gate here can see.
    for key in config::KEYS.into_iter().chain(config::VALUES) {
        let source = if config::VALUES.contains(&key) {
            format!("{key} = ^x\n")
        } else {
            format!("{key} = on\n")
        };
        assert!(
            config::parse(&source).is_ok(),
            "{key:?} is on a list and the parser refuses it"
        );
    }
    assert!(
        config::parse("sidebar = on\n").is_err(),
        "a key on neither list was accepted, so the union above is not what \
         the parser reads"
    );
}

/// The file's first valued setting is read, and reaches the walk.
#[test]
fn a_hide_pattern_is_read_from_the_file() {
    let home = home_with("hide", Some("rail = on\nhide = ^target/|\\.lock$\n"));
    let config = config::from_env(home_env(&home)).expect("a config");
    let hide = config.hide.as_ref().expect("the file set a pattern");

    assert_eq!(hide.as_str(), r"^target/|\.lock$");
    assert!(hide.is_hidden("target/debug/x.o"));
    assert!(hide.is_hidden("deps/pinned.lock"));
    assert!(!hide.is_hidden("src/lib.rs"));

    // And the toggles beside it are untouched, which is what makes the second
    // grammar additive rather than a replacement.
    assert!(config.rail, "the valued key ate the toggle above it");
}

/// Refused by line, before the terminal is taken, on the theme file's reason.
#[test]
fn a_pattern_that_does_not_compile_is_refused_by_line() {
    let err = config::parse("# mine\nrail = on\nhide = ^target/(\n")
        .expect_err("an unclosed group is not a pattern");
    assert!(
        matches!(&err, ConfigError::BadPattern { line, .. } if *line == 3),
        "a bad pattern was accepted, or refused without its line: {err:?}"
    );
    let said = err.to_string();
    assert!(
        said.contains("line 3") && said.contains("hide"),
        "the error names neither the line nor the key: {said}"
    );
    // One sentence, not two. The engine's words arrive bare so this line can
    // frame them, and framing them twice buries the pattern mid-message.
    assert_eq!(
        said.matches("is not a pattern").count(),
        1,
        "the refusal says the same thing twice: {said}"
    );
}

/// A pattern keeps what it was given. Splitting a value into words and rejoining
/// them normalises runs of spaces, which no toggle can notice and a pattern can.
#[test]
fn a_hide_value_keeps_the_spaces_a_toggle_would_lose() {
    let config = config::parse("hide =  ^a  b$ \n").expect("a config");
    assert_eq!(
        config.hide.expect("a pattern").as_str(),
        "^a  b$",
        "the value was rebuilt from its words, so a reader's spacing was changed \
         under them and two different patterns became one"
    );
}

/// One grammar, so a comment ends a value whichever kind of key it is on.
#[test]
fn a_comment_still_ends_a_hide_value() {
    let config = config::parse("hide = ^target/   # everything generated\n").expect("a config");
    assert_eq!(config.hide.expect("a pattern").as_str(), "^target/");

    // A `#` that opens no comment is part of the value, which is the rule the
    // toggles already had: `rail = on# x` is a bad value rather than a good one
    // with a note on it.
    let config = config::parse("hide = ^a#b$\n").expect("a config");
    assert_eq!(config.hide.expect("a pattern").as_str(), "^a#b$");

    // Nothing but a comment is a missing value rather than an empty pattern,
    // which would have hidden every path in the tree.
    assert!(matches!(
        config::parse("hide = # nothing\n"),
        Err(ConfigError::MissingValue { line: 1 })
    ));
}

/// The repeated-key rule reaches the valued key, so `hide` twice names both lines
/// rather than letting the later one win in silence.
#[test]
fn a_pattern_written_twice_is_a_repeat_like_any_other_key() {
    let err = config::parse("hide = ^a\nhide = ^b\n").expect_err("a repeat");
    assert_eq!(
        err,
        ConfigError::RepeatedKey {
            line: 2,
            key: "hide".to_owned(),
            first: 1
        }
    );
}

/// The two lists are what tell a toggle from a valued setting, and a key in both
/// would be set twice with the parser's second reading winning.
#[test]
fn a_toggle_is_never_also_a_valued_setting() {
    assert!(
        !config::KEYS.is_empty() && !config::VALUES.is_empty(),
        "one of the two lists is empty, so the assertion below is vacuous"
    );
    let both: Vec<&str> = config::KEYS
        .into_iter()
        .filter(|key| config::VALUES.contains(key))
        .collect();
    assert!(both.is_empty(), "{both:?} is on both lists");
}

/// `hide` in the file reaches the frame, not just the parser.
///
/// The sibling of the staged gate above, and it exists for the same reason: a key
/// that parses and never reaches the walk is a setting a reader can write, read
/// back in an error message, and never see act.
#[test]
fn a_configured_pattern_is_applied_on_the_first_frame() {
    let scratch = support::Scratch::new("config-hide");
    scratch.write("src/a.rs", "one\ntwo\n");
    scratch.write("target/debug/build.log", "noise\n");
    scratch.git(&["add", "-A"]);
    scratch.git(&["commit", "-m", "init"]);
    scratch.write("src/a.rs", "one\nTWO\n");
    scratch.write("target/debug/build.log", "more noise\n");

    let worktree = scratch.worktree();
    let config = config::parse("hide = ^target/\n").expect("a config");

    let mut frame = worktree.frame();
    vigia::arm_frame(&mut frame, &config);
    frame.advance().expect("advance");

    let drawn: Vec<&str> = frame
        .files()
        .iter()
        .map(|change| change.path.as_str())
        .collect();
    assert_eq!(
        drawn,
        vec!["src/a.rs"],
        "a shell configured with a pattern walked every path anyway, so the key \
         sets a field nothing acts on"
    );
    assert_eq!(frame.hidden(), 1);

    // And a reader with no pattern gets the pane they have always had.
    let mut frame = worktree.frame();
    vigia::arm_frame(&mut frame, &Config::default());
    frame.advance().expect("advance");
    assert_eq!(frame.files().len(), 2, "the fixture changed two files");
    assert_eq!(frame.hidden(), 0);
}

/// A file the reader wrote by hand, with everything a rewrite must not touch.
const HAND_WRITTEN: &str = "\
# the pane I want
rail     = on    # from 134 columns
single   = off

hide = ^target/|\\.lock$
notes = off
";

#[test]
fn a_rewrite_keeps_every_comment_and_every_line_it_does_not_own() {
    let mut config = config::parse(HAND_WRITTEN).expect("the fixture parses");
    config.single = true;
    let out = config::rewrite(HAND_WRITTEN, &config);

    for kept in [
        "# the pane I want",
        "# from 134 columns",
        "hide = ^target/|\\.lock$",
    ] {
        assert!(
            out.contains(kept),
            "the rewrite lost {kept:?}, which is the reader's and not the pane's:\n{out}"
        );
    }
    // The blank line the reader left, in the place they left it.
    assert!(
        out.contains("single   = on\n\nhide"),
        "the rewrite closed up the reader's blank line:\n{out}"
    );
    assert!(
        out.contains("single   = on"),
        "the key the pane owns did not move:\n{out}"
    );
    assert!(
        !out.contains("single   = off"),
        "the old value is still there:\n{out}"
    );
}

#[test]
fn a_rewrite_appends_a_key_the_file_lacks_and_never_writes_hide() {
    let config = config::parse(HAND_WRITTEN).expect("the fixture parses");
    let out = config::rewrite(HAND_WRITTEN, &config);
    let back = config::parse(&out).expect("the rewrite parses");
    assert_eq!(back, config, "the file no longer round trips");

    for key in config::KEYS {
        assert!(
            out.lines().any(|line| line.trim_start().starts_with(key)),
            "the rewrite never writes {key:?}, so a reader cannot see where it stands:\n{out}"
        );
    }
    // `hide` is written exactly once, which is the reader's own line kept rather
    // than a second one appended: the menu owns no pattern and writes none.
    assert_eq!(
        out.lines()
            .filter(|line| line.trim_start().starts_with("hide"))
            .count(),
        1,
        "the rewrite wrote `hide`, which no gesture reaches:\n{out}"
    );
}

#[test]
fn a_file_notepad_saved_survives_a_rewrite() {
    // U+FEFF is `Cf` rather than `White_Space`, so it survives every trim and lands
    // inside the first key. `parse` has stripped it since the first Windows reader
    // hit it. Left in place here, the first key would be unrecognised, appended a
    // second time, and the next launch would refuse the reader's own file with a
    // repeated key it never wrote. The pane would then not start at all.
    let saved = "\u{FEFF}rail = off\nsingle = on\n";
    let config = Config {
        rail: true,
        ..config::parse(saved).expect("the fixture parses")
    };
    let out = config::rewrite(saved, &config);

    assert!(
        out.starts_with('\u{FEFF}'),
        "the mark the reader's editor writes is gone, so their editor puts it back \
         and every flip churns the first line: {out:?}"
    );
    assert_eq!(
        out.matches("rail").count(),
        1,
        "the first key was appended a second time:\n{out}"
    );
    assert!(
        config::parse(&out).expect("the rewrite still parses").rail,
        "the flip did not land"
    );
}

#[test]
fn a_file_written_on_windows_stays_written_on_windows() {
    // Every line ending is the reader's, kept as they wrote it. Converting them
    // would be a whole-file diff on the first flip, and their editor would convert
    // it back on the first save, so the two would churn the file between them
    // forever.
    let theirs = "# mine\r\nrail = off\r\nsingle = on\r\n";
    let config = Config {
        rail: true,
        ..config::parse(theirs).expect("the fixture parses")
    };
    let out = config::rewrite(theirs, &config);

    assert!(
        !out.contains('\n') || out.matches("\r\n").count() == out.matches('\n').count(),
        "a line lost its ending, so the file is half one kind and half the \
         other: {out:?}"
    );
    assert!(
        out.contains("rail = on\r\n"),
        "the flip did not land, or landed without the reader's ending: {out:?}"
    );
    assert!(
        out.contains("# mine\r\n"),
        "the comment lost its ending: {out:?}"
    );
    // And the keys it had to append took the reader's ending too.
    assert!(
        out.contains("persist = off\r\n"),
        "an appended key was written with the wrong ending: {out:?}"
    );
}

#[test]
fn a_save_refuses_a_file_that_no_longer_parses() {
    // Between the launch and the flip a reader may have edited the file by hand.
    // Writing over that is this program deciding what their file should say, and
    // the error it hands back is the parser's own, so the footer names the line.
    let home = support::Scratch::new("config-refuse-bad");
    let path = home.root().join("config");
    std::fs::write(&path, "rail = off\nrail = on\n").expect("seed a file it refuses");

    let refused = config::save(&path, &Config::default()).expect_err("a save over a bad file");
    assert!(
        matches!(&refused, ConfigError::RepeatedKey { key, .. } if key == "rail"),
        "the refusal is not the parser's own: {refused:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("read it back"),
        "rail = off\nrail = on\n",
        "the reader's file was written over anyway"
    );
}

#[test]
fn a_rewrite_adds_no_line_the_reader_did_not_have() {
    // The shapes a real file arrives in, each one a reader's editor rather than a
    // hypothetical. Every one must parse, and none may gain a blank line: a file
    // that grows one on every flip is a file this program is slowly rewriting.
    for (what, source, ends) in [
        // An editor that saved a mark and nothing else.
        ("a mark alone", "\u{FEFF}", "\n"),
        ("a mark and a comment", "\u{FEFF}# mine\n", "\n"),
        // The reader's last line with no ending, which every editor allows.
        ("no trailing ending", "rail = off", "\n"),
        (
            "no trailing ending on Windows",
            "rail = off\r\nsingle = off",
            "\r\n",
        ),
        // Both kinds in one file, which is what a repository shared between two
        // machines produces. The first is the one the appended keys take.
        ("both kinds", "rail = off\r\nsingle = off\n", "\r\n"),
    ] {
        let config = Config::default();
        let out = config::rewrite(source, &config);
        assert!(
            config::parse(&out).is_ok(),
            "{what}: the rewrite made a file the next launch refuses:\n{out:?}"
        );
        assert!(
            !out.contains("\n\n") && !out.contains("\r\n\r\n"),
            "{what}: the rewrite added a blank line:\n{out:?}"
        );
        // And the mark keeps the line it was on. A file that is nothing but a mark
        // has no line to end, so ending one leaves the mark alone on the first row
        // and every flip after it has a blank first line to carry.
        assert!(
            !out.lines()
                .next()
                .is_some_and(|first| first.trim_matches('\u{FEFF}').trim().is_empty()),
            "{what}: the rewrite left the first line with nothing on it:\n{out:?}"
        );
        assert!(
            out.ends_with(&format!("persist = off{ends}")),
            "{what}: the appended keys did not take the {ends:?} the file opens \
             with:\n{out:?}"
        );
        // And a second pass is the first, so a flip a minute later changes nothing
        // but the word the reader flipped.
        assert_eq!(
            config::rewrite(&out, &config),
            out,
            "{what}: the rewrite is not settled after one pass"
        );
    }
}

#[test]
fn a_repeated_key_is_the_readers_mistake_and_the_rewrite_leaves_it() {
    // `parse` refuses a file that sets a key twice, and says which lines. A rewrite
    // must not quietly repair that: the reader has to see the line they wrote when
    // the next launch names it, so only the first occurrence takes the new value and
    // the second is theirs, byte for byte.
    let doubled = "rail = off
single = on
rail = off
";
    let config = Config {
        rail: true,
        ..Config::default()
    };
    let out = config::rewrite(doubled, &config);
    let rails: Vec<&str> = out
        .lines()
        .filter(|line| line.trim_start().starts_with("rail"))
        .collect();
    assert_eq!(
        rails,
        vec!["rail = on", "rail = off"],
        "the rewrite did not leave the reader's second line alone:
{out}"
    );
    // And the file is still the one the next launch will refuse, so nothing was
    // repaired behind their back.
    assert!(
        matches!(
            config::parse(&out),
            Err(ConfigError::RepeatedKey { key, .. }) if key == "rail"
        ),
        "the rewrite made a file `parse` accepts out of one it refuses:
{out}"
    );
}

#[test]
fn a_rewrite_of_a_file_it_wrote_is_the_same_file() {
    // Idempotence, which is what stops a file growing a line every time a reader
    // flips a row.
    let config = config::parse(HAND_WRITTEN).expect("the fixture parses");
    let once = config::rewrite(HAND_WRITTEN, &config);
    let twice = config::rewrite(&once, &config);
    assert_eq!(once, twice, "a second rewrite is not the first:\n{once}");

    // And from nothing: a reader who never had a file gets one that parses back to
    // the same pane. `hide` is the one thing that does not survive, because the menu
    // never writes a pattern and a file written from nothing has none to keep.
    let fresh = config::rewrite("", &config);
    assert_eq!(
        config::parse(&fresh).expect("the fresh file parses"),
        Config {
            hide: None,
            ..config.clone()
        },
        "a file written from nothing does not read back:\n{fresh}"
    );
    assert!(
        !fresh.contains("hide"),
        "a file written from nothing carries a pattern nobody wrote:\n{fresh}"
    );
    assert_eq!(
        config::rewrite(&fresh, &config),
        fresh,
        "rewriting a file it wrote from nothing changed it"
    );
}

#[test]
fn a_save_lands_whole_and_says_which_file_it_could_not_write() {
    let scratch = support::Scratch::new("config-save");
    let path = scratch.root().join("nested/vigia/config");
    let config = Config {
        persist: true,
        rail: true,
        ..Config::default()
    };

    config::save(&path, &config).expect("a save into a directory nobody made");
    assert_eq!(
        config::load(&path).expect("the saved file parses"),
        config,
        "what was saved is not what reads back"
    );
    // Nothing beside it: temp-and-rename leaves no temp behind.
    let beside: Vec<String> = std::fs::read_dir(path.parent().expect("a parent"))
        .expect("read the directory")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        beside,
        vec!["config".to_owned()],
        "the save left a file behind"
    );

    // A directory where the file should be is a write that cannot land, and the
    // error names the path rather than only the reason.
    let blocked = scratch.root().join("blocked");
    std::fs::create_dir_all(blocked.join("config")).expect("make the blocking directory");
    let refused = config::save(&blocked.join("config"), &config).expect_err("a blocked save");
    assert!(
        refused.to_string().contains("config"),
        "the refusal does not name the file: {refused}"
    );
    // And it says why before it says where. The footer takes a notice's tail off at
    // the width it has, and a path is longer than the reason on every machine.
    assert!(
        !refused
            .to_string()
            .starts_with(&blocked.display().to_string()),
        "the refusal leads with the path, so a narrow pane cuts the reason off it: {refused}"
    );
}

/// A link is the reader's own arrangement, and a rename lands on the entry
/// rather than on what it points at.
#[test]
fn a_save_writes_through_a_link_rather_than_over_it() {
    // A dotfile manager owns the entry in the config directory and points it at a
    // file in the tree it tracks. Replacing the entry orphans that file, and the
    // pane then writes somewhere the reader does not keep, with nothing on screen
    // to say the arrangement is gone.
    let scratch = support::Scratch::new("config-link");
    let kept = scratch.root().join("dotfiles/config");
    std::fs::create_dir_all(kept.parent().expect("a parent")).expect("the tracked tree");
    std::fs::write(&kept, "# mine\nrail = off\n").expect("seed the tracked file");

    let link = scratch.root().join("config");
    if !support::linked_file(&kept, &link) {
        return;
    }

    let config = Config {
        rail: true,
        ..Config::default()
    };
    config::save(&link, &config).expect("a save through the link");

    assert!(
        std::fs::symlink_metadata(&link)
            .expect("the link")
            .is_symlink(),
        "the save replaced the reader's link with a file of its own"
    );
    let written = std::fs::read_to_string(&kept).expect("the tracked file");
    assert!(
        written.starts_with("# mine\n") && written.contains("rail = on"),
        "the file the link points at was not the file that was written: {written:?}"
    );
    assert_eq!(
        config::load(&link).expect("the saved file parses"),
        config,
        "what was saved is not what reads back through the link"
    );
}

/// A link laid down before what it points at exists is still the reader's link.
#[test]
fn a_save_through_a_link_with_nothing_behind_it_makes_what_it_names() {
    // A dotfiles tree whose links are made before it is checked out, which is the
    // one shape the path resolving above cannot answer: nothing to resolve to, and
    // the fallback it would otherwise take is the link itself.
    // Both shapes a link is written in. A tool that manages a tree writes the
    // relative one as often as the whole path, and only one of them is resolved
    // against the directory the link itself sits in.
    for (what, from_its_own_directory) in [("a whole path", false), ("a relative path", true)] {
        let scratch = support::Scratch::new(&format!("config-dangling-{from_its_own_directory}"));
        let kept = scratch.root().join("dotfiles/config");
        let link = scratch.root().join("config");
        let relative = std::path::Path::new("dotfiles").join("config");
        let names: &std::path::Path = if from_its_own_directory {
            &relative
        } else {
            &kept
        };
        if !support::linked_file(names, &link) {
            return;
        }
        assert!(
            !kept.exists(),
            "{what}: the fixture made the file it is about to write"
        );

        let config = Config {
            rail: true,
            ..Config::default()
        };
        config::save(&link, &config).expect("a save through a link with nothing behind it");

        assert!(
            std::fs::symlink_metadata(&link)
                .expect("the link")
                .is_symlink(),
            "{what}: the save replaced the reader's link with a file of its own"
        );
        assert!(
            kept.exists(),
            "{what}: the file the link names was not the file that was made"
        );
        assert_eq!(
            config::load(&link).expect("the saved file parses"),
            config,
            "{what}: what was saved is not what reads back through the link"
        );
    }
}
