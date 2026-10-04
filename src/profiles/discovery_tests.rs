use super::tests::{entry, labels, with_home};
use super::*;

fn discovery_cases() -> Vec<(&'static str, AppEntry)> {
    let chrome = ["/usr/bin/google-chrome-stable", "%U"];
    vec![
        ("a", entry("google-chrome", "Chrome", &chrome, &[])),
        (
            "a",
            entry(
                "brave-origin",
                "Brave Origin",
                &["/usr/bin/brave-origin-stable", "%U"],
                &[],
            ),
        ),
        ("a", entry("firefox", "Firefox", &["firefox", "%u"], &[])),
        (
            "a",
            entry(
                "google-chrome",
                "Chrome",
                &[
                    "/usr/bin/google-chrome-stable",
                    "--profile-directory=Default",
                    "%U",
                ],
                &[],
            ),
        ),
        (
            "a",
            entry(
                "google-chrome",
                "Chrome",
                &["/opt/other/chromeish", "%U"],
                &[],
            ),
        ),
        (
            "a",
            entry(
                "firefox",
                "Firefox",
                &["env", "MOZ_X=1", "firefox", "%u"],
                &[],
            ),
        ),
        (
            "a",
            entry("unknown-browser", "Other", &["other", "%u"], &[]),
        ),
        (
            "ambiguous",
            entry("firefox", "Firefox", &["firefox", "%u"], &[]),
        ),
        ("malformed", entry("google-chrome", "Chrome", &chrome, &[])),
    ]
}

#[test]
fn profiles_for_lists_every_profile_including_a_single_one() {
    let found: Vec<Vec<String>> = discovery_cases()
        .iter()
        .map(|(home, e)| {
            with_home(home, |env| {
                profiles_for(e, env).into_iter().map(|p| p.key).collect()
            })
        })
        .collect();
    assert_eq!(found[0], ["Default", "Profile 1", "Profile 2"]);
    assert_eq!(found[1], ["Default"]);
    assert_eq!(found[2], ["default-release", "Work Stuff"]);
}

#[test]
fn profiles_for_applies_the_guards_of_tiles_for() {
    let found: Vec<usize> = discovery_cases()
        .iter()
        .skip(3)
        .map(|(home, e)| with_home(home, |env| profiles_for(e, env).len()))
        .collect();
    assert_eq!(
        found, [0; 6],
        "a selected profile, an unknown executable, an unsupported launcher, an unknown app, an ambiguous \
         store and a malformed store each discover nothing"
    );
}

#[test]
fn tiles_for_offers_profile_tiles_exactly_for_the_profiles_profiles_for_finds() {
    for (home, e) in discovery_cases() {
        with_home(home, |env| {
            let profiles = profiles_for(&e, env);
            let shown: Vec<Option<Profile>> =
                tiles_for(&e, env).into_iter().map(|t| t.profile).collect();
            let expected: Vec<Option<Profile>> = if profiles.len() >= 2 {
                profiles.into_iter().map(Some).collect()
            } else {
                vec![None]
            };
            assert_eq!(shown, expected, "{} in {home}", e.id);
        });
    }
}

#[test]
fn link_tiles_is_tiles_for_each_entry_in_order_then_disambiguated() {
    let entries = [
        entry(
            "firefox",
            "Firefox",
            &["env", "MOZ_X=1", "firefox", "%u"],
            &[],
        ),
        entry(
            "google-chrome",
            "Chrome",
            &["/usr/bin/google-chrome-stable", "%U"],
            &[],
        ),
        entry(
            "org.mozilla.firefox",
            "Firefox",
            &[
                "/usr/bin/flatpak",
                "run",
                "--command=firefox",
                "--file-forwarding",
                "org.mozilla.firefox",
                "@@u",
                "%u",
                "@@",
            ],
            &[],
        ),
    ];
    with_home("ambiguous", |env| {
        let mut expected: Vec<Tile> = entries.iter().flat_map(|e| tiles_for(e, env)).collect();
        disambiguate(&mut expected);
        let tiles = link_tiles(&entries, env);
        assert_eq!(tiles, expected);
        assert_eq!(labels(&tiles), ["Firefox", "Chrome", "Firefox (Flatpak)"]);
    });
}

#[test]
fn a_store_file_that_is_a_pipe_leaves_the_store_unusable_at_once() {
    let home = tempfile::tempdir().unwrap();
    let store = home.path().join(".config/google-chrome");
    std::fs::create_dir_all(&store).unwrap();
    rustix::fs::mknodat(
        rustix::fs::CWD,
        store.join("Local State"),
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::from_raw_mode(0o600),
        0,
    )
    .unwrap();
    let chrome = entry(
        "google-chrome",
        "Chrome",
        &["/usr/bin/google-chrome-stable", "%U"],
        &[],
    );
    // Nothing writes to the pipe, so a plain read of it never returns.
    let (sender, answer) = std::sync::mpsc::channel();
    let home = home.path().to_owned();
    std::thread::spawn(move || {
        let config = home.join(".config");
        let env = Env {
            home: &home,
            config_home: &config,
            view: &crate::index::Native,
        };
        let found = profiles_for(&chrome, &env);
        let paths = store_paths(&chrome, &env).expect("Chrome's profiles are discovered");
        let _ = sender.send((found.is_empty(), paths.profiles.is_err()));
    });
    assert_eq!(
        answer.recv_timeout(std::time::Duration::from_secs(5)),
        Ok((true, true))
    );
}

#[test]
fn a_listed_profile_whose_folder_is_gone_gets_no_tile() {
    let home = tempfile::tempdir().unwrap();
    let store = home.path().join(".config/google-chrome");
    std::fs::create_dir_all(store.join("Default")).unwrap();
    std::fs::write(
        store.join("Local State"),
        r#"{"profile":{"info_cache":{"Default":{"name":"Me"},"Profile 3":{"name":"Gone"}}}}"#,
    )
    .unwrap();
    let chrome = entry(
        "google-chrome",
        "Chrome",
        &["/usr/bin/google-chrome-stable", "%U"],
        &[],
    );
    let config = home.path().join(".config");
    let env = Env {
        home: home.path(),
        config_home: &config,
        view: &crate::index::Native,
    };
    let names: Vec<String> = profiles_for(&chrome, &env)
        .into_iter()
        .map(|profile| profile.label)
        .collect();
    assert_eq!(names, ["Me"]);
}
