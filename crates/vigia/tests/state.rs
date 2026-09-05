//! `SPEC.md` §11.2 B21: where the notes store lives.

use std::path::{Path, PathBuf};

use vigia::state_root;

#[test]
fn the_state_root_follows_xdg_then_home_and_localappdata_on_windows() {
    let env = |vars: &[(&str, &str)]| {
        let owned: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |name: &str| {
            owned
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        }
    };
    // Absolute on every platform, which is what the XDG rule turns on.
    let absolute_str = std::env::temp_dir()
        .join("xdg-state")
        .to_string_lossy()
        .into_owned();
    let absolute = PathBuf::from(&absolute_str);

    assert_eq!(
        state_root(
            false,
            env(&[
                ("XDG_STATE_HOME", absolute_str.as_str()),
                ("HOME", "/home/r")
            ])
        ),
        Some(absolute.join("vigia"))
    );
    // A relative value is invalid and ignored, per the specification.
    assert_eq!(
        state_root(
            false,
            env(&[("XDG_STATE_HOME", "state"), ("HOME", "/home/r")])
        ),
        Some(PathBuf::from("/home/r/.local/state/vigia"))
    );
    // Set but empty is unset.
    assert_eq!(
        state_root(false, env(&[("XDG_STATE_HOME", "  "), ("HOME", "/home/r")])),
        Some(PathBuf::from("/home/r/.local/state/vigia"))
    );
    assert_eq!(
        state_root(false, env(&[("USERPROFILE", "/u/r")])),
        Some(PathBuf::from("/u/r/.local/state/vigia"))
    );
    assert_eq!(state_root(false, env(&[])), None);
    assert_eq!(
        state_root(
            true,
            env(&[
                ("LOCALAPPDATA", "C:\\Users\\r\\AppData\\Local"),
                ("HOME", "/h")
            ])
        ),
        Some(
            Path::new("C:\\Users\\r\\AppData\\Local")
                .join("vigia")
                .join("state")
        )
    );
    assert_eq!(state_root(true, env(&[("HOME", "/h")])), None);
    assert_eq!(state_root(true, env(&[("LOCALAPPDATA", " ")])), None);
}
