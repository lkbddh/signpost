use super::*;

fn cache(order: &str) -> String {
    format!(
        r#"{{"profile":{{"info_cache":{{"Default":{{"name":"Personal"}},"Profile 1":{{"name":"Work"}},"Profile 2":{{"name":"Café"}}}}{order}}}}}"#
    )
}

fn dirs(p: &[ChromiumProfile]) -> Vec<&str> {
    p.iter().map(|x| x.dir.as_str()).collect()
}

#[test]
fn chromium_valid_order_is_used() {
    let p = chromium_profiles(&cache(
        r#","profiles_order":["Profile 1","Profile 2","Default"]"#,
    ))
    .unwrap();
    assert_eq!(dirs(&p), ["Profile 1", "Profile 2", "Default"]);
    assert_eq!(p[1].name, "Café");
}

#[test]
fn chromium_invalid_orders_fall_back_to_name_sort() {
    for order in [
        "",
        r#","profiles_order":["Default","Default","Profile 1","Profile 2"]"#,
        r#","profiles_order":["Default","Profile 1","Profile 2","Ghost"]"#,
        r#","profiles_order":["Default","Profile 1"]"#,
        r#","profiles_order":"nope""#,
        r#","profiles_order":["Profile 1",null,"Profile 2","Default"]"#,
    ] {
        let p = chromium_profiles(&cache(order)).unwrap();
        assert_eq!(
            dirs(&p),
            ["Profile 2", "Default", "Profile 1"],
            "order {order}"
        );
    }
}

#[test]
fn chromium_name_ties_break_on_dir() {
    let p = chromium_profiles(
        r#"{"profile":{"info_cache":{"B":{"name":"Same"},"A":{"name":"Same"}}}}"#,
    )
    .unwrap();
    assert_eq!(dirs(&p), ["A", "B"]);
}

#[test]
fn chromium_malformed() {
    assert!(matches!(
        chromium_profiles("{not json"),
        Err(ProfileError::Json(_))
    ));
    assert_eq!(
        chromium_profiles(r#"{"profile":{}}"#),
        Err(ProfileError::NoInfoCache)
    );
}

#[test]
fn firefox_profiles_resolve_paths_in_file_order() {
    let ini = "[General]\nStartWithLastProfile=1\n\n[Profile1]\nName=Work Stuff\nIsRelative=1\nPath=abcd.work\n\n\
                   [Install4F96D1932A9F858E]\nDefault=x\n\n[Profile0]\nName=default-release\nIsRelative=0\nPath=/srv/ff/rel\n\n\
                   [Profile2]\nPath=nameless\n";
    assert_eq!(
        firefox_profiles(ini, Path::new("/home/u/.config/mozilla/firefox")),
        vec![
            FirefoxProfile {
                name: "Work Stuff".into(),
                path: "/home/u/.config/mozilla/firefox/abcd.work".into()
            },
            FirefoxProfile {
                name: "default-release".into(),
                path: "/srv/ff/rel".into()
            },
        ]
    );
}

#[test]
fn ini_ignores_comments_and_junk() {
    assert_eq!(
        parse_ini("; c\njunk\n[A]\nk = v\n# c\n[B]\n"),
        vec![
            ("A".into(), vec![("k".into(), "v".into())]),
            ("B".into(), vec![])
        ]
    );
}

use crate::index::AppAction;

fn home(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/homes")
        .join(name)
}

pub(super) fn entry(id: &str, name: &str, exec: &[&str], actions: &[(&str, &[&str])]) -> AppEntry {
    AppEntry {
        id: id.into(),
        name: name.into(),
        icon: Some(id.into()),
        exec: Some(exec.iter().map(|s| (*s).to_owned()).collect()),
        exec_error: None,
        try_exec: None,
        work_dir: None,
        terminal: false,
        dbus_activatable: false,
        no_display: false,
        hidden: false,
        mime_types: vec!["x-scheme-handler/https".into()],
        actions: actions
            .iter()
            .map(|(id, exec)| AppAction {
                id: (*id).into(),
                name: format!("{id} label"),
                exec: exec.iter().map(|s| (*s).to_owned()).collect(),
            })
            .collect(),
        file: PathBuf::from(format!("/usr/share/applications/{id}.desktop")),
    }
}

pub(super) fn with_home<T>(name: &str, f: impl FnOnce(&Env) -> T) -> T {
    let h = home(name);
    let c = h.join(".config");
    f(&Env {
        home: &h,
        config_home: &c,
        view: &crate::index::Native,
    })
}

pub(super) fn labels(t: &[Tile]) -> Vec<&str> {
    t.iter().map(|x| x.label.as_str()).collect()
}

#[test]
fn helium_profiles_become_ordered_tiles_under_both_its_ids() {
    with_home("a", |env| {
        for id in ["helium", "net.imput.helium"] {
            let t = tiles_for(&entry(id, "Helium", &["helium", "%U"], &[]), env);
            assert_eq!(labels(&t), ["personal", "projects"]);
            assert_eq!(t[1].profile.as_ref().unwrap().key, "Profile 1");
            assert_eq!(t[1].app_name, "Helium");
        }
    });
}

/// A `%` in `--user-data-dir` could be a field code as well as a letter of the folder's name, so the store is not
/// read and the browser is one plain tile; a profile launch never has to pass such a folder on.
#[test]
fn a_user_data_dir_with_a_percent_sign_is_one_plain_tile() {
    let tmp = tempfile::tempdir().unwrap();
    for name in ["100%%data", "%u"] {
        // A readable store, so only the refusal keeps it from being read.
        let root = tmp.path().join(name);
        for dir in ["Default", "Profile 1"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        std::fs::write(
            root.join("Local State"),
            r#"{"profile":{"info_cache":{"Default":{"name":"one"},"Profile 1":{"name":"two"}}}}"#,
        )
        .unwrap();
        let written = format!("--user-data-dir={}", root.display());
        let chrome = entry(
            "google-chrome",
            "Google Chrome",
            &["/usr/bin/google-chrome-stable", &written, "%U"],
            &[],
        );
        with_home("a", |env| {
            let t = tiles_for(&chrome, env);
            assert_eq!(labels(&t), ["Google Chrome"], "{written}");
            assert!(t[0].profile.is_none(), "{written}");
        });
    }
}

#[test]
fn chrome_profiles_become_ordered_tiles_including_alias_id() {
    with_home("a", |env| {
        for id in ["google-chrome", "com.google.Chrome"] {
            let t = tiles_for(
                &entry(
                    id,
                    "Google Chrome",
                    &["/usr/bin/google-chrome-stable", "%U"],
                    &[],
                ),
                env,
            );
            assert_eq!(labels(&t), ["acme", "travel", "work"]);
            assert_eq!(t[1].profile.as_ref().unwrap().key, "Profile 1");
            assert_eq!(t[1].app_name, "Google Chrome");
        }
    });
}

/// A browser that takes links through its D-Bus `Open` method, its Exec dropping them, is one plain tile: a profile
/// or private window would run that Exec without the link.
#[test]
fn a_browser_whose_exec_drops_the_link_is_one_plain_tile_that_opens_it_over_dbus() {
    with_home("a", |env| {
        let mut chrome = entry(
            "google-chrome",
            "Google Chrome",
            &["/usr/bin/google-chrome-stable"],
            &[],
        );
        chrome.dbus_activatable = true;
        assert_eq!(profiles_for(&chrome, env), Vec::new());
        let t = tiles_for(&chrome, env);
        assert_eq!(labels(&t), ["Google Chrome"]);
        assert!(t[0].profile.is_none() && t[0].actions.is_empty(), "{t:?}");
        assert_eq!(
            crate::launch::plan(&chrome, &t[0], None, "https://x/", env, "x"),
            Ok(crate::launch::Plan::DBus {
                desktop_id: "google-chrome".into()
            })
        );
    });
}

#[test]
fn single_profile_browser_is_one_plain_tile() {
    with_home("a", |env| {
        let t = tiles_for(
            &entry(
                "brave-origin",
                "Brave Origin",
                &["/usr/bin/brave-origin-stable", "%U"],
                &[],
            ),
            env,
        );
        assert_eq!(labels(&t), ["Brave Origin"]);
        assert!(t[0].profile.is_none());
    });
}

#[test]
fn firefox_native_uses_xdg_root_flatpak_uses_var_app_root() {
    with_home("a", |env| {
        let native = tiles_for(&entry("firefox", "Firefox", &["firefox", "%u"], &[]), env);
        assert_eq!(labels(&native), ["default-release", "Work Stuff"]);
        let flat = tiles_for(
            &entry(
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
            env,
        );
        assert_eq!(labels(&flat), ["Flat One", "Flat Two"]);
        assert!(flat.iter().all(|t| t.flatpak));
    });
}

#[test]
fn fallbacks_to_one_plain_tile() {
    with_home("ambiguous", |env| {
        assert_eq!(
            labels(&tiles_for(
                &entry("firefox", "Firefox", &["firefox", "%u"], &[]),
                env
            )),
            ["Firefox"]
        );
    });
    with_home("malformed", |env| {
        assert_eq!(
            labels(&tiles_for(
                &entry(
                    "google-chrome",
                    "Chrome",
                    &["/usr/bin/google-chrome-stable", "%U"],
                    &[]
                ),
                env
            )),
            ["Chrome"]
        );
    });
    with_home("a", |env| {
        assert_eq!(
            labels(&tiles_for(
                &entry(
                    "firefox",
                    "Firefox",
                    &["env", "MOZ_X=1", "firefox", "%u"],
                    &[]
                ),
                env
            )),
            ["Firefox"]
        );
        assert_eq!(
            labels(&tiles_for(
                &entry(
                    "google-chrome",
                    "Chrome",
                    &["chrome", "--profile-directory=Default", "%U"],
                    &[]
                ),
                env
            )),
            ["Chrome"]
        );
        assert_eq!(
            labels(&tiles_for(
                &entry("unknown-browser", "Other", &["other", "%u"], &[]),
                env
            )),
            ["Other"]
        );
    });
}

#[test]
fn exec_selecting_a_profile_is_one_plain_tile() {
    // Known executables, so only the selector check can produce the plain tile.
    with_home("a", |env| {
        for (id, name, exec) in [
            (
                "google-chrome",
                "Chrome",
                &[
                    "/usr/bin/google-chrome-stable",
                    "--profile-directory=Default",
                    "%U",
                ][..],
            ),
            (
                "firefox",
                "Firefox",
                &["firefox", "-P", "default", "%u"][..],
            ),
        ] {
            let t = tiles_for(&entry(id, name, exec, &[]), env);
            assert_eq!(labels(&t), [name], "{exec:?}");
            assert!(t[0].profile.is_none());
        }
    });
}

#[test]
fn actions_only_take_url_capable_desktop_actions_plus_private() {
    with_home("a", |env| {
        let chrome = tiles_for(
            &entry(
                "google-chrome",
                "Chrome",
                &["/usr/bin/google-chrome-stable", "%U"],
                &[
                    ("new-window", &["/usr/bin/google-chrome-stable"]),
                    (
                        "new-private-window",
                        &["/usr/bin/google-chrome-stable", "--incognito"],
                    ),
                ],
            ),
            env,
        );
        assert_eq!(chrome[0].actions, vec![TileAction::Private]);
        let ff = tiles_for(
            &entry(
                "firefox",
                "Firefox",
                &["firefox", "%u"],
                &[("new-window", &["firefox", "--new-window", "%u"])],
            ),
            env,
        );
        assert_eq!(
            ff[0].actions,
            vec![
                TileAction::Desktop {
                    id: "new-window".into(),
                    label: "new-window label".into()
                },
                TileAction::Private
            ]
        );
        let wrapped = tiles_for(
            &entry("firefox", "Firefox", &["env", "A=1", "firefox", "%u"], &[]),
            env,
        );
        assert!(
            wrapped[0].actions.is_empty(),
            "private flag cannot be inserted through a wrapper"
        );
        let other = tiles_for(&entry("other", "Other", &["other", "%u"], &[]), env);
        assert_eq!(other[0].actions, []);
    });
}

#[test]
fn actions_that_would_run_the_url_as_shell_code_are_not_offered() {
    with_home("a", |env| {
        let t = tiles_for(
            &entry(
                "other",
                "Other",
                &["other", "%u"],
                &[
                    ("embedded", &["sh", "-c", "other --new-window %u"]),
                    ("positional", &["sh", "-c", r#"other "$1""#, "sh", "%u"]),
                    ("plain", &["other", "--new-window", "%u"]),
                ],
            ),
            env,
        );
        let ids: Vec<&str> = t[0]
            .actions
            .iter()
            .map(|a| match a {
                TileAction::Desktop { id, .. } => id.as_str(),
                TileAction::Private => "private",
            })
            .collect();
        assert_eq!(ids, ["plain"]);
    });
}

#[test]
fn flags_per_adapter() {
    let p = Profile {
        key: "Work Stuff".into(),
        label: "Work Stuff".into(),
        dir: PathBuf::new(),
        color: None,
    };
    let c = Profile {
        key: "Profile 1".into(),
        label: "x".into(),
        dir: PathBuf::new(),
        color: None,
    };
    let adapter = |id: &str| adapter_for(id).expect(id);
    assert_eq!(
        adapter("firefox").store.profile_args(&p),
        ["-P", "Work Stuff"]
    );
    for chromium in ["google-chrome", "microsoft-edge"] {
        assert_eq!(
            adapter(chromium).store.profile_args(&c),
            ["--profile-directory=Profile 1"]
        );
    }
    assert_eq!(adapter("google-chrome").private, "--incognito");
    assert_eq!(adapter("microsoft-edge").private, "--inprivate");
    assert_eq!(adapter("firefox").private, "--private-window");
}

#[test]
fn missing_ambiguous_or_empty_firefox_stores_are_reported() {
    let a = adapter_for("firefox").unwrap();
    let exec: Vec<String> = vec!["firefox".into(), "%u".into()];
    let ff = entry("firefox", "Firefox", &["firefox", "%u"], &[]);
    with_home("ambiguous", |env| {
        assert!(matches!(
            discover(a, &Launcher::Native, env, &exec),
            Err(ProfileError::Ambiguous(_))
        ));
    });
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join(".config");
    let env = Env {
        home: tmp.path(),
        config_home: &config,
        view: &crate::index::Native,
    };
    assert!(
        matches!(
            discover(a, &Launcher::Native, &env, &exec),
            Err(ProfileError::Io(_))
        ),
        "no profiles.ini anywhere"
    );
    assert_eq!(labels(&tiles_for(&ff, &env)), ["Firefox"]);
    let store = config.join("mozilla/firefox");
    std::fs::create_dir_all(&store).unwrap();
    std::fs::write(
        store.join("profiles.ini"),
        "[General]\nStartWithLastProfile=1\n",
    )
    .unwrap();
    assert!(
        matches!(
            discover(a, &Launcher::Native, &env, &exec),
            Err(ProfileError::NoProfiles(_))
        ),
        "a store listing no profile"
    );
    assert_eq!(labels(&tiles_for(&ff, &env)), ["Firefox"]);
}

#[test]
fn revalidation_detects_vanished_profiles() {
    with_home("a", |env| {
        let a = adapter_for("firefox").unwrap();
        let exec: Vec<String> = vec!["firefox".into(), "%u".into()];
        let ok = discover(a, &Launcher::Native, env, &exec)
            .unwrap()
            .remove(1);
        assert!(revalidate(a, &Launcher::Native, env, &exec, &ok));
        let gone = Profile {
            dir: env.home.join("nope"),
            ..ok.clone()
        };
        assert!(!revalidate(a, &Launcher::Native, env, &exec, &gone));
        let renamed = Profile {
            key: "Renamed".into(),
            ..ok
        };
        assert!(!revalidate(a, &Launcher::Native, env, &exec, &renamed));
    });
}

#[test]
fn unknown_executable_is_plain_and_user_data_dir_selects_the_root() {
    with_home("a", |env| {
        assert_eq!(
            labels(&tiles_for(
                &entry(
                    "google-chrome",
                    "Chrome",
                    &["/opt/custom/run-chrome", "%U"],
                    &[]
                ),
                env
            )),
            ["Chrome"]
        );
        let custom = home("custom-root").join("chrome-data");
        let flag = format!("--user-data-dir={}", custom.display());
        let joined = tiles_for(
            &entry(
                "google-chrome",
                "Chrome",
                &["/usr/bin/google-chrome-stable", &flag, "%U"],
                &[],
            ),
            env,
        );
        assert_eq!(labels(&joined), ["Alt One", "Alt Two"]);
        let split = tiles_for(
            &entry(
                "google-chrome",
                "Chrome",
                &[
                    "google-chrome",
                    "--user-data-dir",
                    custom.to_str().unwrap(),
                    "%U",
                ],
                &[],
            ),
            env,
        );
        assert_eq!(labels(&split), ["Alt One", "Alt Two"]);
        let wrong_flatpak = tiles_for(
            &entry(
                "google-chrome",
                "Chrome",
                &["flatpak", "run", "org.other.App", "%U"],
                &[],
            ),
            env,
        );
        assert_eq!(labels(&wrong_flatpak), ["Chrome"]);
    });
}

#[test]
fn profile_tiles_hide_actions_with_a_different_root() {
    with_home("a", |env| {
        let t = tiles_for(
            &entry(
                "google-chrome",
                "Chrome",
                &["/usr/bin/google-chrome-stable", "%U"],
                &[
                    (
                        "same",
                        &["/usr/bin/google-chrome-stable", "--new-window", "%U"],
                    ),
                    (
                        "elsewhere",
                        &["/usr/bin/google-chrome-stable", "--user-data-dir=/x", "%U"],
                    ),
                ],
            ),
            env,
        );
        assert_eq!(t.len(), 3);
        assert_eq!(
            t[0].actions,
            vec![
                TileAction::Desktop {
                    id: "same".into(),
                    label: "same label".into()
                },
                TileAction::Private
            ]
        );
    });
}

#[test]
fn duplicate_plain_labels_mark_the_flatpak_one() {
    let base = Tile {
        app_id: "a".into(),
        app_name: "Firefox".into(),
        label: "Firefox".into(),
        icon: None,
        profile: None,
        flatpak: false,
        actions: vec![],
    };
    let mut tiles = vec![
        base.clone(),
        Tile {
            app_id: "b".into(),
            flatpak: true,
            ..base
        },
    ];
    disambiguate(&mut tiles);
    assert_eq!(labels(&tiles), ["Firefox", "Firefox (Flatpak)"]);
}

#[test]
fn native_and_flatpak_profiles_of_the_same_name_are_told_apart() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join(".config");
    for root in [
        config.join("mozilla/firefox"),
        tmp.path()
            .join(".var/app/org.mozilla.firefox/.mozilla/firefox"),
    ] {
        for dir in ["work", "home"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        std::fs::write(
            root.join("profiles.ini"),
            "[Profile0]\nName=Work\nIsRelative=1\nPath=work\n\n[Profile1]\nName=Home\nIsRelative=1\nPath=home\n",
        )
        .unwrap();
    }
    let env = Env {
        home: tmp.path(),
        config_home: &config,
        view: &crate::index::Native,
    };
    let native = entry("firefox", "Firefox", &["firefox", "%u"], &[]);
    let flatpak = entry(
        "org.mozilla.firefox",
        "Firefox",
        &["flatpak", "run", "org.mozilla.firefox", "%u"],
        &[],
    );
    let mut tiles: Vec<Tile> = [native, flatpak]
        .iter()
        .flat_map(|e| tiles_for(e, &env))
        .collect();
    disambiguate(&mut tiles);
    assert_eq!(
        labels(&tiles),
        ["Work", "Home", "Work (Flatpak)", "Home (Flatpak)"]
    );
}

#[test]
fn profile_tiles_hide_actions_that_switch_profile_store() {
    with_home("a", |env| {
        let t = tiles_for(
            &entry(
                "firefox",
                "Firefox",
                &["firefox", "%u"],
                &[
                    ("same", &["firefox", "--new-window", "%u"]),
                    (
                        "flatpak",
                        &[
                            "/usr/bin/flatpak",
                            "run",
                            "--command=firefox",
                            "org.mozilla.firefox",
                            "%u",
                        ],
                    ),
                    ("other-exe", &["other-browser", "--new-window", "%u"]),
                    ("wrapped", &["env", "A=1", "firefox", "%u"]),
                ],
            ),
            env,
        );
        assert_eq!(labels(&t), ["default-release", "Work Stuff"]);
        for tile in &t {
            assert_eq!(
                tile.actions,
                vec![
                    TileAction::Desktop {
                        id: "same".into(),
                        label: "same label".into()
                    },
                    TileAction::Private
                ]
            );
        }
    });
}

#[test]
fn profile_tiles_keep_actions_that_inherit_the_main_root() {
    // Main Exec with a data-root override: an action without one inherits it, an explicit
    // different one does not.
    with_home("a", |env| {
        let flag = format!(
            "--user-data-dir={}",
            home("custom-root").join("chrome-data").display()
        );
        let t = tiles_for(
            &entry(
                "google-chrome",
                "Chrome",
                &["/usr/bin/google-chrome-stable", &flag, "%U"],
                &[
                    (
                        "same",
                        &["/usr/bin/google-chrome-stable", &flag, "--new-window", "%U"],
                    ),
                    (
                        "inherits",
                        &["/usr/bin/google-chrome-stable", "--new-window", "%U"],
                    ),
                    (
                        "elsewhere",
                        &["/usr/bin/google-chrome-stable", "--user-data-dir=/x", "%U"],
                    ),
                ],
            ),
            env,
        );
        assert_eq!(labels(&t), ["Alt One", "Alt Two"]);
        let kept = |id: &str| TileAction::Desktop {
            id: id.into(),
            label: format!("{id} label"),
        };
        assert_eq!(
            t[0].actions,
            vec![kept("same"), kept("inherits"), TileAction::Private]
        );
    });
}

#[test]
fn beta_and_unstable_chrome_never_use_the_stable_store() {
    with_home("a", |env| {
        for exe in ["google-chrome-beta", "google-chrome-unstable"] {
            let exec = format!("/usr/bin/{exe}");
            let t = tiles_for(&entry("google-chrome", "Chrome", &[&exec, "%U"], &[]), env);
            assert_eq!(labels(&t), ["Chrome"], "{exe}");
            assert!(t[0].profile.is_none() && t[0].actions.is_empty());
        }
        // A beta action is not offered on the stable profile tiles.
        let t = tiles_for(
            &entry(
                "google-chrome",
                "Chrome",
                &["/usr/bin/google-chrome-stable", "%U"],
                &[(
                    "beta",
                    &["/usr/bin/google-chrome-beta", "--new-window", "%U"],
                )],
            ),
            env,
        );
        assert_eq!(labels(&t), ["acme", "travel", "work"]);
        assert_eq!(t[0].actions, vec![TileAction::Private]);
    });
}

#[test]
fn relative_data_root_is_one_plain_tile() {
    // Lib tests run with the package root as cwd, so this relative root would resolve to a real store.
    let rel = "tests/fixtures/homes/custom-root/chrome-data";
    assert!(Path::new(rel).join("Local State").is_file());
    with_home("a", |env| {
        let joined = format!("--user-data-dir={rel}");
        for exec in [
            vec!["/usr/bin/google-chrome-stable", joined.as_str(), "%U"],
            vec!["google-chrome", "--user-data-dir", rel, "%U"],
        ] {
            let t = tiles_for(&entry("google-chrome", "Chrome", &exec, &[]), env);
            assert_eq!(labels(&t), ["Chrome"], "{exec:?}");
            let exec: Vec<String> = exec.into_iter().map(str::to_owned).collect();
            let a = adapter_for("google-chrome").unwrap();
            assert!(discover(a, &Launcher::Native, env, &exec).is_err());
        }
    });
}

#[test]
fn data_root_with_a_percent_is_one_plain_tile() {
    // `%c` in an Exec token is a field code, not the literal directory that exists on disk.
    with_home("percent", |env| {
        let root = home("percent").join("chrome-%c");
        assert!(root.join("Local State").is_file());
        let flag = format!("--user-data-dir={}", root.display());
        let exec = ["/usr/bin/google-chrome-stable", flag.as_str(), "%U"];
        let t = tiles_for(&entry("google-chrome", "Chrome", &exec, &[]), env);
        assert_eq!(labels(&t), ["Chrome"]);
        let exec: Vec<String> = exec.into_iter().map(str::to_owned).collect();
        let a = adapter_for("google-chrome").unwrap();
        assert!(matches!(
            discover(a, &Launcher::Native, env, &exec),
            Err(ProfileError::Ambiguous(_))
        ));
    });
}

#[test]
fn flatpak_data_root_override_is_one_plain_tile() {
    with_home("a", |env| {
        let flag = format!(
            "--user-data-dir={}",
            home("custom-root").join("chrome-data").display()
        );
        let t = tiles_for(
            &entry(
                "com.google.Chrome",
                "Chrome",
                &[
                    "/usr/bin/flatpak",
                    "run",
                    "--command=google-chrome-stable",
                    "com.google.Chrome",
                    &flag,
                    "%U",
                ],
                &[],
            ),
            env,
        );
        assert_eq!(labels(&t), ["Chrome"]);
        assert!(t[0].flatpak && t[0].profile.is_none());
    });
}

#[test]
fn duplicate_launch_names_are_one_plain_tile() {
    with_home("dupnames", |env| {
        let t = tiles_for(&entry("firefox", "Firefox", &["firefox", "%u"], &[]), env);
        assert_eq!(labels(&t), ["Firefox"]);
        let a = adapter_for("firefox").unwrap();
        let exec: Vec<String> = vec!["firefox".into(), "%u".into()];
        assert!(discover(a, &Launcher::Native, env, &exec).is_err());
        let p = Profile {
            key: "Work".into(),
            label: "Work".into(),
            dir: env.config_home.join("mozilla/firefox/work.one"),
            color: None,
        };
        assert!(!revalidate(a, &Launcher::Native, env, &exec, &p));
    });
}

const ORANGE: Rgb = Rgb {
    r: 0xFF,
    g: 0x80,
    b: 0x00,
};

/// Colors decoded from one `Local State` holding a profile per seed (at most ten, so name order is seed order).
fn seeded(seeds: &[&str]) -> Vec<Option<Rgb>> {
    let entries: Vec<String> = seeds
        .iter()
        .enumerate()
        .map(|(i, seed)| format!(r#""P{i}":{{"name":"P{i}","profile_color_seed":{seed}}}"#))
        .collect();
    let json = format!(
        r#"{{"profile":{{"info_cache":{{{}}}}}}}"#,
        entries.join(",")
    );
    let profiles = chromium_profiles(&json).unwrap();
    profiles.iter().map(|p| p.color).collect()
}

fn colors(profiles: &[Profile]) -> Vec<Option<Rgb>> {
    profiles.iter().map(|p| p.color).collect()
}

fn chrome_exec() -> Vec<String> {
    vec!["/usr/bin/google-chrome-stable".into(), "%U".into()]
}

#[test]
fn chromium_seed_decodes_signed_argb() {
    let rgb = |r, g, b| Some(Rgb { r, g, b });
    assert_eq!(
        seeded(&["-32768", "-16711936", "-151840", "-1", "-16777216"]),
        [
            Some(ORANGE),
            rgb(0x00, 0xFF, 0x00),
            rgb(0xFD, 0xAE, 0xE0),
            rgb(0xFF, 0xFF, 0xFF),
            rgb(0x00, 0x00, 0x00),
        ]
    );
}

#[test]
fn chromium_seed_without_full_alpha_has_no_color() {
    // Alpha 0, 0x7F, 0x80 and 0xFE, then the all-zero and the minimum value; the last profile is the control.
    assert_eq!(
        seeded(&[
            "16744448",
            "2147450880",
            "-2130739200",
            "-16809984",
            "0",
            "-2147483648",
            "-32768",
        ]),
        [None, None, None, None, None, None, Some(ORANGE)]
    );
}

#[test]
fn chromium_malformed_seeds_have_no_color_and_spare_their_siblings() {
    for bad in [
        r#""-32768""#,
        "-32768.0",
        "true",
        "null",
        "[-32768]",
        r#"{"argb":-32768}"#,
        "4294967295",
        "-2147483649",
        "18446744073709551615",
    ] {
        assert_eq!(seeded(&[bad, "-32768"]), [None, Some(ORANGE)], "{bad}");
    }
}

#[test]
fn chromium_missing_seed_has_no_color_and_no_other_field_stands_in() {
    let json = r#"{"profile":{"info_cache":{
            "Default":{"name":"Plain"},
            "Profile 1":{"name":"Themed","profile_highlight_color":-32768,
                "default_avatar_fill_color":-32768,"default_avatar_stroke_color":-32768,"avatar_icon":"chrome://theme/IDR_PROFILE_AVATAR_26"}
        }}}"#;
    let profiles = chromium_profiles(json).unwrap();
    assert_eq!(
        profiles.iter().map(|p| p.color).collect::<Vec<_>>(),
        [None, None]
    );
}

#[test]
fn chromium_color_follows_its_profile_through_any_order() {
    // Seeds sort the opposite way to names, so ordering by color would be visible.
    let cache = |order: &str| {
        format!(
            r#"{{"profile":{{"info_cache":{{"A":{{"name":"Alpha","profile_color_seed":-1}},"B":{{"name":"Beta","profile_color_seed":-16777216}}}}{order}}}}}"#
        )
    };
    let pairs = |order: &str| -> Vec<(String, Option<Rgb>)> {
        chromium_profiles(&cache(order))
            .unwrap()
            .into_iter()
            .map(|p| (p.dir, p.color))
            .collect()
    };
    let white = Some(Rgb {
        r: 255,
        g: 255,
        b: 255,
    });
    let black = Some(Rgb { r: 0, g: 0, b: 0 });
    assert_eq!(pairs(""), [("A".into(), white), ("B".into(), black)]);
    assert_eq!(
        pairs(r#","profiles_order":["B","A"]"#),
        [("B".into(), black), ("A".into(), white)]
    );
}

#[test]
fn rgb_round_trips_through_json() {
    let json = serde_json::to_string(&ORANGE).unwrap();
    assert_eq!(json, r#"{"r":255,"g":128,"b":0}"#);
    assert_eq!(serde_json::from_str::<Rgb>(&json).unwrap(), ORANGE);
}

#[test]
fn discover_carries_colors_for_native_flatpak_and_custom_root_stores() {
    with_home("a", |env| {
        let chrome = adapter_for("google-chrome").unwrap();
        let native = discover(chrome, &Launcher::Native, env, &chrome_exec()).unwrap();
        assert_eq!(
            colors(&native),
            [
                Some(ORANGE),
                Some(Rgb {
                    r: 0xFD,
                    g: 0xAE,
                    b: 0xE0
                }),
                None
            ]
        );
        let flatpak = Launcher::Flatpak {
            app_id: "com.google.Chrome".into(),
        };
        let sandboxed = discover(chrome, &flatpak, env, &[]).unwrap();
        assert_eq!(
            colors(&sandboxed),
            [
                Some(Rgb {
                    r: 0x00,
                    g: 0xFF,
                    b: 0x00
                }),
                Some(Rgb {
                    r: 0xFF,
                    g: 0xFF,
                    b: 0xFF
                })
            ]
        );
        let flag = format!(
            "--user-data-dir={}",
            home("custom-root").join("chrome-data").display()
        );
        let exec = vec!["/usr/bin/google-chrome-stable".into(), flag, "%U".into()];
        let custom = discover(chrome, &Launcher::Native, env, &exec).unwrap();
        assert_eq!(
            colors(&custom),
            [
                Some(Rgb {
                    r: 0x00,
                    g: 0xFF,
                    b: 0x00
                }),
                Some(Rgb {
                    r: 0x00,
                    g: 0x00,
                    b: 0x00
                })
            ]
        );
    });
}

#[test]
fn firefox_profiles_have_no_color() {
    with_home("a", |env| {
        let firefox = adapter_for("firefox").unwrap();
        let exec: Vec<String> = vec!["firefox".into(), "%u".into()];
        let native = discover(firefox, &Launcher::Native, env, &exec).unwrap();
        let flatpak = Launcher::Flatpak {
            app_id: "org.mozilla.firefox".into(),
        };
        let sandboxed = discover(firefox, &flatpak, env, &[]).unwrap();
        assert_eq!(native.len() + sandboxed.len(), 4);
        assert!(native.iter().chain(&sandboxed).all(|p| p.color.is_none()));
    });
}

#[test]
fn profile_tiles_carry_their_color() {
    with_home("a", |env| {
        let t = tiles_for(
            &entry(
                "google-chrome",
                "Chrome",
                &["/usr/bin/google-chrome-stable", "%U"],
                &[],
            ),
            env,
        );
        let tile_colors: Vec<Option<Rgb>> = t
            .iter()
            .map(|t| t.profile.as_ref().unwrap().color)
            .collect();
        assert_eq!(tile_colors[0], Some(ORANGE));
        assert_eq!(tile_colors[2], None);
    });
}

#[test]
fn same_labelled_profiles_are_disambiguated_whatever_their_colors() {
    with_home("a", |env| {
        let native = entry(
            "google-chrome",
            "Chrome",
            &["/usr/bin/google-chrome-stable", "%U"],
            &[],
        );
        let flatpak = entry(
            "com.google.Chrome",
            "Chrome",
            &[
                "/usr/bin/flatpak",
                "run",
                "--command=google-chrome-stable",
                "com.google.Chrome",
                "%U",
            ],
            &[],
        );
        let mut tiles: Vec<Tile> = [native, flatpak]
            .iter()
            .flat_map(|e| tiles_for(e, env))
            .collect();
        disambiguate(&mut tiles);
        assert_eq!(
            labels(&tiles),
            ["acme", "travel", "work", "acme (Flatpak)", "scratch"]
        );
        let native_acme = tiles[0].profile.as_ref().unwrap().color;
        let flatpak_acme = tiles[3].profile.as_ref().unwrap().color;
        assert_ne!(native_acme, flatpak_acme, "the fixture colors differ");
    });
}

#[test]
fn a_recolored_profile_still_revalidates() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join(".config");
    let store = config.join("google-chrome");
    std::fs::create_dir_all(store.join("Default")).unwrap();
    std::fs::create_dir_all(store.join("Profile 1")).unwrap();
    let write = |profile_1: &str| {
        std::fs::write(
            store.join("Local State"),
            format!(
                r#"{{"profile":{{"info_cache":{{"Default":{{"name":"One","profile_color_seed":-1}},"Profile 1":{{"name":"Two"{profile_1}}}}}}}}}"#
            ),
        )
        .unwrap();
    };
    let env = Env {
        home: tmp.path(),
        config_home: &config,
        view: &crate::index::Native,
    };
    let chrome = adapter_for("google-chrome").unwrap();
    let exec = chrome_exec();
    let discover_two = || {
        discover(chrome, &Launcher::Native, &env, &exec)
            .unwrap()
            .remove(1)
    };
    write(r#","profile_color_seed":-32768"#);
    let before = discover_two();
    assert_eq!(before.color, Some(ORANGE));
    assert!(revalidate(chrome, &Launcher::Native, &env, &exec, &before));
    write(r#","profile_color_seed":-16711936"#);
    assert!(revalidate(chrome, &Launcher::Native, &env, &exec, &before));
    assert_ne!(discover_two().color, before.color);
    write("");
    assert!(revalidate(chrome, &Launcher::Native, &env, &exec, &before));
    assert_eq!(discover_two().color, None);
}

/// A Flatpak's view of a host whose files the sandbox shows under `root` (`os` for the OS trees, `rest` for the
/// others).
fn host_view(root: &Path) -> crate::host::HostEnv {
    crate::host::HostEnv::parse(b"HOME=/home/u\0", Path::new("/sandbox/home")).mounted_under(root)
}

fn put(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn a_flatpak_reads_profiles_where_the_sandbox_shows_the_hosts_and_keeps_host_paths() {
    let root = tempfile::tempdir().unwrap();
    let view = host_view(root.path());
    let (os, rest) = (root.path().join("os"), root.path().join("rest"));
    let chrome = adapter_for("google-chrome").unwrap();
    // A config home in a host OS tree, with the runtime's own folder beside it as a decoy.
    put(
        &os.join("etc/cfg/google-chrome/Local State"),
        r#"{"profile":{"info_cache":{"Default":{"name":"Personal"}}}}"#,
    );
    put(
        &rest.join("etc/cfg/google-chrome/Local State"),
        r#"{"profile":{"info_cache":{"Default":{"name":"Decoy"}}}}"#,
    );
    std::fs::create_dir_all(os.join("etc/cfg/google-chrome/Default")).unwrap();
    let env = Env {
        home: Path::new("/home/u"),
        config_home: Path::new("/etc/cfg"),
        view: &view,
    };
    let found = discover(chrome, &Launcher::Native, &env, &chrome_exec()).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].label, "Personal",
        "the host's store, not the runtime's"
    );
    assert_eq!(found[0].dir, Path::new("/etc/cfg/google-chrome/Default"));
    assert!(revalidate(
        chrome,
        &Launcher::Native,
        &env,
        &chrome_exec(),
        &found[0]
    ));

    // A profile root set in Exec, read where the sandbox shows it.
    put(
        &rest.join("home/u/chrome-data/Local State"),
        r#"{"profile":{"info_cache":{"Default":{"name":"Custom"}}}}"#,
    );
    std::fs::create_dir_all(rest.join("home/u/chrome-data/Default")).unwrap();
    let custom = [
        "/usr/bin/google-chrome-stable",
        "--user-data-dir=/home/u/chrome-data",
        "%U",
    ]
    .map(String::from)
    .to_vec();
    let found = discover(chrome, &Launcher::Native, &env, &custom).unwrap();
    assert_eq!(found[0].label, "Custom");
    assert_eq!(found[0].dir, Path::new("/home/u/chrome-data/Default"));

    // One where the sandbox only has a folder of its own is not read.
    put(
        &rest.join("tmp/chrome/Local State"),
        r#"{"profile":{"info_cache":{"Default":{"name":"Decoy"}}}}"#,
    );
    let refused = [
        "/usr/bin/google-chrome-stable",
        "--user-data-dir=/tmp/chrome",
        "%U",
    ]
    .map(String::from)
    .to_vec();
    assert!(matches!(
        discover(chrome, &Launcher::Native, &env, &refused),
        Err(ProfileError::Io(_))
    ));
}

#[test]
fn a_flatpak_checks_an_absolute_firefox_profile_where_the_sandbox_shows_it() {
    let root = tempfile::tempdir().unwrap();
    let view = host_view(root.path());
    let rest = root.path().join("rest");
    let firefox = adapter_for("firefox").unwrap();
    put(
        &rest.join("home/u/.config/mozilla/firefox/profiles.ini"),
        "[Profile0]\nName=Work\nIsRelative=0\nPath=/srv/ff/work\n",
    );
    let env = Env {
        home: Path::new("/home/u"),
        config_home: Path::new("/home/u/.config"),
        view: &view,
    };
    let exec = ["firefox", "%u"].map(String::from).to_vec();
    assert_eq!(
        discover(firefox, &Launcher::Native, &env, &exec).unwrap(),
        [],
        "not there yet"
    );
    std::fs::create_dir_all(rest.join("srv/ff/work")).unwrap();
    let found = discover(firefox, &Launcher::Native, &env, &exec).unwrap();
    assert_eq!(found[0].dir, Path::new("/srv/ff/work"));
    assert!(revalidate(
        firefox,
        &Launcher::Native,
        &env,
        &exec,
        &found[0]
    ));
    std::fs::remove_dir(rest.join("srv/ff/work")).unwrap();
    assert!(
        !revalidate(firefox, &Launcher::Native, &env, &exec, &found[0]),
        "gone again"
    );
}

#[test]
fn an_absolute_icon_is_shown_from_where_the_view_reads_it_and_a_name_as_it_is() {
    let root = tempfile::tempdir().unwrap();
    let view = host_view(root.path());
    assert_eq!(
        shown_icon(Some("/usr/share/pixmaps/app.png"), &view),
        Some(
            root.path()
                .join("os/usr/share/pixmaps/app.png")
                .display()
                .to_string()
        )
    );
    assert_eq!(
        shown_icon(Some("firefox"), &view),
        Some("firefox".to_owned())
    );
    assert_eq!(
        shown_icon(Some("/tmp/app.png"), &view),
        None,
        "the sandbox's own /tmp"
    );
    assert_eq!(shown_icon(None, &view), None);
    assert_eq!(
        shown_icon(Some("/usr/share/pixmaps/app.png"), &crate::index::Native),
        Some("/usr/share/pixmaps/app.png".to_owned())
    );
}

#[test]
fn a_flatpak_leaves_out_profiles_whose_folders_it_cannot_reach() {
    let root = tempfile::tempdir().unwrap();
    let view = host_view(root.path());
    let rest = root.path().join("rest");
    let env = Env {
        home: Path::new("/home/u"),
        config_home: Path::new("/home/u/.config"),
        view: &view,
    };
    // Firefox: one profile in the home, one at an absolute path in the sandbox's own /tmp.
    put(
        &rest.join("home/u/.config/mozilla/firefox/profiles.ini"),
        "[Profile0]\nName=Home\nIsRelative=1\nPath=home\n\n[Profile1]\nName=Away\nIsRelative=0\nPath=/tmp/ff\n",
    );
    std::fs::create_dir_all(rest.join("home/u/.config/mozilla/firefox/home")).unwrap();
    std::fs::create_dir_all(rest.join("tmp/ff")).unwrap();
    let firefox = adapter_for("firefox").unwrap();
    let exec = ["firefox", "%u"].map(String::from).to_vec();
    let found = discover(firefox, &Launcher::Native, &env, &exec).unwrap();
    let names: Vec<&str> = found.iter().map(|p| p.label.as_str()).collect();
    assert_eq!(names, ["Home"], "the /tmp profile is left out");

    // Chrome: a profile folder that links into /tmp.
    put(
        &rest.join("home/u/.config/google-chrome/Local State"),
        r#"{"profile":{"info_cache":{"Default":{"name":"Personal"},"Profile 1":{"name":"Away"}}}}"#,
    );
    std::fs::create_dir_all(rest.join("home/u/.config/google-chrome/Default")).unwrap();
    std::os::unix::fs::symlink(
        "/tmp/chrome-away",
        rest.join("home/u/.config/google-chrome/Profile 1"),
    )
    .unwrap();
    std::fs::create_dir_all(rest.join("tmp/chrome-away")).unwrap();
    let chrome = adapter_for("google-chrome").unwrap();
    let found = discover(chrome, &Launcher::Native, &env, &chrome_exec()).unwrap();
    let names: Vec<&str> = found.iter().map(|p| p.label.as_str()).collect();
    assert_eq!(
        names,
        ["Personal"],
        "the folder linked into /tmp is left out"
    );
}

#[test]
fn a_name_shared_with_an_unreachable_profile_is_still_ambiguous() {
    let root = tempfile::tempdir().unwrap();
    let view = host_view(root.path());
    let rest = root.path().join("rest");
    put(
        &rest.join("home/u/.config/mozilla/firefox/profiles.ini"),
        "[Profile0]\nName=Work\nIsRelative=1\nPath=work\n\n[Profile1]\nName=Work\nIsRelative=0\nPath=/tmp/work\n\n\
         [Profile2]\nName=Personal\nIsRelative=1\nPath=personal\n",
    );
    for dir in [
        "home/u/.config/mozilla/firefox/work",
        "home/u/.config/mozilla/firefox/personal",
        "tmp/work",
    ] {
        std::fs::create_dir_all(rest.join(dir)).unwrap();
    }
    let env = Env {
        home: Path::new("/home/u"),
        config_home: Path::new("/home/u/.config"),
        view: &view,
    };
    let firefox = adapter_for("firefox").unwrap();
    let exec = ["firefox", "%u"].map(String::from).to_vec();
    assert!(
        matches!(
            discover(firefox, &Launcher::Native, &env, &exec),
            Err(ProfileError::Ambiguous(_))
        ),
        "Firefox on the host still sees two profiles called Work"
    );
}

#[test]
fn a_profile_tile_offers_no_action_that_already_picks_a_profile() {
    with_home("a", |env| {
        let e = entry(
            "firefox",
            "Firefox",
            &["firefox", "%u"],
            &[
                ("new-window", &["firefox", "--new-window", "%u"]),
                (
                    "work-window",
                    &["firefox", "-P", "Work", "--new-window", "%u"],
                ),
            ],
        );
        let tiles = tiles_for(&e, env);
        assert!(tiles.len() > 1, "a tile per profile: {tiles:?}");
        for tile in &tiles {
            let ids: Vec<&str> = tile
                .actions
                .iter()
                .filter_map(|action| match action {
                    TileAction::Desktop { id, .. } => Some(id.as_str()),
                    TileAction::Private => None,
                })
                .collect();
            assert_eq!(ids, ["new-window"], "{}", tile.label);
        }
    });
}
