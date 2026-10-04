use std::path::PathBuf;

use super::*;
use crate::index::{AppAction, AppEntry, MimeLists, Registry};
use crate::profiles::{Env, Tile, link_tiles};
use crate::setup::{DESKTOP_FILE, MimeBaseline, Record};
use crate::test_support::logs::logged;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn registry() -> Registry {
    Registry::load(
        &[
            fixtures().join("applications-user"),
            fixtures().join("applications-system"),
        ],
        &["en".into()],
    )
}

fn candidates(registry: &Registry) -> Vec<AppEntry> {
    let lists = MimeLists::load(&[
        fixtures().join("mimeapps/high.list"),
        fixtures().join("mimeapps/low.list"),
    ]);
    registry
        .link_candidates("x-scheme-handler/https", &lists)
        .into_iter()
        .cloned()
        .collect()
}

fn rows_and_tiles(entries: &[AppEntry]) -> (Vec<AppRow>, Vec<Tile>) {
    let home = fixtures().join("homes/a");
    let config_home = home.join(".config");
    let env = Env {
        home: &home,
        config_home: &config_home,
        view: &crate::index::Native,
    };
    (inventory(entries, &env), link_tiles(entries, &env))
}

fn variant(base: &AppEntry, id: &str, exec: &[&str]) -> AppEntry {
    AppEntry {
        id: id.to_owned(),
        name: id.to_owned(),
        exec: Some(exec.iter().map(|t| (*t).to_owned()).collect()),
        actions: Vec::new(),
        ..base.clone()
    }
}

fn tile_numbers(row: &AppRow) -> Vec<Option<usize>> {
    row.profiles.iter().map(|p| p.tile).collect()
}

fn row<'a>(rows: &'a [AppRow], id: &str) -> &'a AppRow {
    rows.iter().find(|r| r.id == id).unwrap()
}

const FLATPAK_FIREFOX: [&str; 8] = [
    "/usr/bin/flatpak",
    "run",
    "--command=firefox",
    "--file-forwarding",
    "org.mozilla.firefox",
    "@@u",
    "%u",
    "@@",
];

#[test]
fn inventory_follows_the_picker_order_and_numbers_profiles_like_its_tiles() {
    let entries = candidates(&registry());
    let (rows, tiles) = rows_and_tiles(&entries);
    let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "google-chrome",
            "org.example.DbusOnly",
            "firefox",
            "escaped",
            "term"
        ]
    );
    let numbers: Vec<Vec<Option<usize>>> = rows.iter().map(tile_numbers).collect();
    assert_eq!(
        numbers,
        [
            vec![Some(1), Some(2), Some(3)],
            vec![],
            vec![Some(5), Some(6)],
            vec![],
            vec![]
        ]
    );
    for (row, profile) in rows
        .iter()
        .flat_map(|r| r.profiles.iter().map(move |p| (r, p)))
    {
        let tile = &tiles[profile.tile.unwrap() - 1];
        assert_eq!(tile.app_id, row.id);
        assert_eq!(tile.profile.as_ref().map(|p| &p.key), Some(&profile.key));
    }
}

#[test]
fn rows_carry_the_apps_name_icon_and_each_profiles_label_directory_and_seed() {
    let (rows, _) = rows_and_tiles(&candidates(&registry()));
    let chrome = row(&rows, "google-chrome");
    assert_eq!(
        (chrome.name.as_str(), chrome.icon.as_deref()),
        ("Google Chrome", Some("google-chrome"))
    );
    let home = fixtures().join("homes/a/.config/google-chrome");
    let described: Vec<_> = chrome
        .profiles
        .iter()
        .map(|p| (p.key.as_str(), p.label.as_str(), p.dir.clone(), p.seed))
        .collect();
    assert_eq!(
        described,
        [
            (
                "Default",
                "acme",
                home.join("Default"),
                Some(Rgb {
                    r: 0xFF,
                    g: 0x80,
                    b: 0x00
                })
            ),
            (
                "Profile 1",
                "travel",
                home.join("Profile 1"),
                Some(Rgb {
                    r: 0xFD,
                    g: 0xAE,
                    b: 0xE0
                })
            ),
            ("Profile 2", "work", home.join("Profile 2"), None),
        ]
    );
    assert_eq!(row(&rows, "firefox").name, "Firefox (user)");
}

#[test]
fn a_single_profile_app_keeps_its_profile_with_its_plain_tile_number() {
    let registry = registry();
    let firefox = registry.get("firefox").unwrap();
    let brave = variant(
        firefox,
        "brave-origin",
        &["/usr/bin/brave-origin-stable", "%U"],
    );
    let (rows, tiles) = rows_and_tiles(&[brave, firefox.clone()]);
    let [brave_row, firefox_row] = rows.as_slice() else {
        panic!("one row per app: {rows:?}");
    };
    assert_eq!(tile_numbers(brave_row), [Some(1)]);
    assert_eq!(brave_row.profiles[0].key, "Default");
    assert_eq!(brave_row.profiles[0].label, "Personal");
    assert!(tiles[0].profile.is_none(), "the app has one plain tile");
    assert_eq!(tile_numbers(firefox_row), [Some(2), Some(3)]);
}

#[test]
fn apps_without_discoverable_profiles_list_none() {
    let (rows, _) = rows_and_tiles(&candidates(&registry()));
    for id in ["org.example.DbusOnly", "escaped", "term"] {
        assert!(row(&rows, id).profiles.is_empty(), "{id}");
    }
}

#[test]
fn the_flatpak_flag_follows_the_launcher() {
    let registry = registry();
    let firefox = registry.get("firefox").unwrap();
    let flatpak = variant(firefox, "org.mozilla.firefox", &FLATPAK_FIREFOX);
    let (rows, _) = rows_and_tiles(&[firefox.clone(), flatpak]);
    assert_eq!(
        rows.iter().map(|r| r.flatpak).collect::<Vec<_>>(),
        [false, true]
    );
    assert_eq!(tile_numbers(&rows[0]), [Some(1), Some(2)]);
    assert_eq!(tile_numbers(&rows[1]), [Some(3), Some(4)]);
}

#[test]
fn private_and_desktop_actions_come_from_the_tiles_not_from_adapter_recognition() {
    let registry = registry();
    let firefox = registry.get("firefox").unwrap();
    let chrome = registry.get("google-chrome").unwrap();
    let foreign_root = AppAction {
        id: "other-root".into(),
        name: "Other root".into(),
        exec: [
            "/usr/bin/google-chrome-stable",
            "--user-data-dir=/elsewhere",
            "%U",
        ]
        .map(String::from)
        .to_vec(),
    };
    let wrapped = AppEntry {
        actions: firefox.actions.clone(),
        ..variant(firefox, "firefox", &["env", "MOZ_X=1", "firefox", "%u"])
    };
    let chrome_with_foreign_root = AppEntry {
        actions: vec![foreign_root],
        ..chrome.clone()
    };
    let (rows, _) = rows_and_tiles(&[
        firefox.clone(),
        chrome_with_foreign_root,
        registry.get("escaped").unwrap().clone(),
    ]);
    assert!(
        rows[0].private,
        "a supported launcher offers a private window"
    );
    assert_eq!(
        rows[0].desktop_actions,
        ["New Window", "New Private Window"]
    );
    assert!(rows[1].private);
    assert!(
        rows[1].desktop_actions.is_empty(),
        "a profile tile drops an action aimed at another data root"
    );
    assert!(!rows[2].private, "an unknown app has no private window");
    assert_eq!(rows[2].desktop_actions, ["Act"]);

    let (rows, _) = rows_and_tiles(&[wrapped]);
    assert!(
        !rows[0].private,
        "an adapter id with an unsupported launcher offers no private window"
    );
    assert_eq!(
        rows[0].desktop_actions,
        ["New Window", "New Private Window"]
    );
}

#[test]
fn the_inventory_discovers_each_apps_store_once() {
    let tmp = tempfile::tempdir().unwrap();
    let config_home = tmp.path().join(".config");
    let store = config_home.join("google-chrome");
    std::fs::create_dir_all(&store).unwrap();
    std::fs::write(store.join("Local State"), "not json").unwrap();
    let env = Env {
        home: tmp.path(),
        config_home: &config_home,
        view: &crate::index::Native,
    };
    let registry = registry();
    let chrome = registry.get("google-chrome").unwrap().clone();
    let log = logged(|| {
        let rows = inventory(&[chrome], &env);
        assert!(rows[0].profiles.is_empty(), "the unusable store lists none");
    });
    assert_eq!(log.matches("profile store unusable").count(), 1, "{log}");
}

const SIGNPOST: &str = "com.lkbddh.signpost";
const OVERRIDING_LIST: &str = "/home/u/.config/cosmic-mimeapps.list";
const KNOWN_APPS: [(&str, &str, &str); 3] = [
    ("firefox.desktop", "Firefox", "firefox"),
    ("brave.desktop", "Brave Origin", "brave-origin"),
    ("google-chrome.desktop", "Google Chrome", "google-chrome"),
];

fn describe(id: &str) -> Option<Identity> {
    let (_, name, icon) = KNOWN_APPS.iter().find(|(known, ..)| *known == id)?;
    Some(Identity {
        id: id.to_owned(),
        name: (*name).to_owned(),
        icon: Some((*icon).to_owned()),
    })
}

fn handler(id: Option<&str>) -> setup::Handler {
    setup::Handler {
        apps: id.map(|id| vec![format!("{id}.desktop")]),
        source: id.map(|_| PathBuf::from("/etc/xdg/mimeapps.list")),
    }
}

fn valid() -> Record {
    Record::Valid(MimeBaseline {
        path: PathBuf::from("/home/u/.config/mimeapps.list"),
        resolved: None,
        existed: true,
        added_defaults_group: None,
        http: None,
        https: Some(vec!["firefox.desktop".into()]),
        effective_http: None,
        effective_https: Some(vec!["firefox.desktop".into()]),
    })
}

fn snap(http: Option<&str>, https: Option<&str>, record: Record) -> Snapshot {
    Snapshot {
        http: handler(http),
        https: handler(https),
        record,
    }
}

fn model_of(snapshot: &Snapshot, last: Option<&(Op, Result<(), OpError>)>) -> Model {
    model(snapshot, &describe, last)
}

/// Fluent wraps substituted values in direction-isolation marks.
pub(super) fn plain(text: &str) -> String {
    text.replace(['\u{2068}', '\u{2069}'], "")
}

#[test]
fn with_signpost_for_both_schemes_the_row_offers_restore_defaults_while_there_is_a_record() {
    let m = model_of(&snap(Some(SIGNPOST), Some(SIGNPOST), valid()), None);
    assert_eq!(m.handler, HandlerView::Signpost);
    assert_eq!((m.action, m.can_run), (Action::Restore, true));
    assert_eq!(plain(&m.action.label()), "Restore defaults");
    assert_eq!(m.record_error, None);

    let m = model_of(&snap(Some(SIGNPOST), Some(SIGNPOST), Record::Absent), None);
    assert_eq!(
        (m.action, m.can_run),
        (Action::Restore, false),
        "in its place, with nothing to restore"
    );
}

#[test]
fn every_other_state_offers_use_signpost_for_both_schemes() {
    let firefox = Some("firefox");
    let states = [
        (firefox, firefox),
        (Some("brave"), Some(SIGNPOST)),
        (Some(SIGNPOST), Some("brave")),
        (Some(SIGNPOST), None),
        (firefox, Some("brave")),
        (firefox, None),
        (None, Some("brave")),
        (None, None),
    ];
    for (http, https) in states {
        for record in [Record::Absent, valid()] {
            let m = model_of(&snap(http, https, record), None);
            assert_eq!(
                (m.action, m.can_run),
                (Action::UseSignpost, true),
                "{http:?} {https:?}"
            );
            assert_eq!(plain(&m.action.label()), "Use Signpost");
        }
    }
}

#[test]
fn the_first_entry_of_each_scheme_decides_its_handler() {
    let mut s = snap(Some("firefox"), Some("firefox"), Record::Absent);
    s.http.apps = Some(vec![DESKTOP_FILE.into(), "firefox.desktop".into()]);
    s.https.apps = Some(vec![DESKTOP_FILE.into()]);
    assert_eq!(model_of(&s, None).handler, HandlerView::Signpost);
}

#[test]
fn the_row_names_one_app_or_says_mixed_or_none() {
    let value = |http: Option<&str>, https: Option<&str>| {
        plain(
            &model_of(&snap(http, https, Record::Absent), None)
                .handler
                .value(),
        )
    };
    assert_eq!(value(Some(SIGNPOST), Some(SIGNPOST)), "Signpost");
    assert_eq!(value(Some("firefox"), Some("firefox")), "Firefox");
    assert_eq!(value(Some("brave"), Some(SIGNPOST)), "Mixed");
    assert_eq!(value(Some(SIGNPOST), None), "Mixed");
    assert_eq!(value(None, None), "None");

    let mut empty_entry = snap(None, None, Record::Absent);
    empty_entry.http.apps = Some(Vec::new());
    assert_eq!(model_of(&empty_entry, None).handler, HandlerView::None);
}

#[test]
fn an_unreadable_record_explains_itself_and_offers_no_setup_that_would_fail() {
    let reason = "restore record /state/signpost/restore.json is unreadable: EOF while parsing";
    let unreadable = || Record::Unreadable(reason.into());
    for (handler, unreadable) in [
        (Some("firefox"), unreadable()),
        (Some(SIGNPOST), unreadable()),
    ] {
        let m = model_of(&snap(handler, handler, unreadable), None);
        let row = m.record_error.expect("the record's band");
        assert_eq!(plain(&row.title), "Saved defaults couldn't be read");
        assert_eq!(
            row.body.as_deref().map(plain),
            Some(
                "Signpost can't change your web browser or restore the previous one until this is fixed. \
                 Repair the file named in the details, or remove it to forget the previous browser."
                    .to_owned()
            )
        );
        assert_eq!(
            row.details,
            [reason],
            "the path and the error, behind Show details"
        );
        assert!(!row.offers_restore);
        assert!(
            !m.can_run,
            "setup and restore would both fail on the record"
        );
    }

    let m = model_of(&snap(Some(SIGNPOST), Some(SIGNPOST), valid()), None);
    assert_eq!(m.record_error, None);
}

#[test]
fn a_setup_the_record_stopped_shows_the_records_band_once_and_never_its_path_as_a_title() {
    let reason = "restore record /state/signpost/restore.json is unreadable: EOF while parsing";
    let stopped = (Op::Set, Err(OpError::Record(reason.into())));

    let m = model_of(
        &snap(
            Some("firefox"),
            Some("firefox"),
            Record::Unreadable(reason.into()),
        ),
        Some(&stopped),
    );
    assert!(m.record_error.is_some());
    assert_eq!(m.error_row, None, "the record's own band says it already");

    // Read again after the failure, the record is fine: the failure still explains itself the same way.
    let m = model_of(
        &snap(Some("firefox"), Some("firefox"), valid()),
        Some(&stopped),
    );
    assert_eq!(m.record_error, None);
    let row = m.error_row.expect("the failure's band");
    assert_eq!(plain(&row.title), "Saved defaults couldn't be read");
    assert_eq!(row.details, [reason]);
}

#[test]
fn an_unknown_desktop_id_is_shown_by_its_id() {
    let m = model_of(
        &snap(Some("mystery"), Some("mystery"), Record::Absent),
        None,
    );
    assert_eq!(
        m.handler,
        HandlerView::App(Identity {
            id: "mystery.desktop".into(),
            name: "mystery".into(),
            icon: None
        })
    );
}

#[test]
fn use_signpost_is_the_suggested_action_and_restore_is_not() {
    assert!(Action::UseSignpost.is_suggested());
    assert!(!Action::Restore.is_suggested());
}

fn overridden() -> Snapshot {
    let mut s = snap(Some(SIGNPOST), Some("google-chrome"), valid());
    s.https = setup::Handler {
        apps: Some(vec!["google-chrome.desktop".into(), "brave.desktop".into()]),
        source: Some(OVERRIDING_LIST.into()),
    };
    s
}

#[test]
fn a_failed_verification_lists_the_blocking_entries_from_the_snapshot() {
    let m = model_of(&overridden(), Some(&(Op::Set, Err(OpError::Verify))));
    let row = m.error_row.unwrap();
    assert_eq!(row.title, "Another settings file overrides this change");
    assert_eq!(
        row.body.as_deref(),
        Some("You can put back the defaults you had before Signpost.")
    );
    assert_eq!(
        row.details,
        [format!(
            "{OVERRIDING_LIST}: x-scheme-handler/https=google-chrome.desktop;brave.desktop"
        )]
    );
    assert!(row.offers_restore);
}

#[test]
fn every_scheme_that_is_not_signpost_is_listed_in_http_then_https_order() {
    let mut s = overridden();
    s.http = setup::Handler {
        apps: Some(vec!["firefox.desktop".into()]),
        source: Some("/etc/xdg/mimeapps.list".into()),
    };
    let row = model_of(&s, Some(&(Op::Set, Err(OpError::Verify))))
        .error_row
        .unwrap();
    assert_eq!(
        row.details,
        [
            "/etc/xdg/mimeapps.list: x-scheme-handler/http=firefox.desktop".to_owned(),
            format!(
                "{OVERRIDING_LIST}: x-scheme-handler/https=google-chrome.desktop;brave.desktop"
            ),
        ]
    );
}

#[test]
fn restore_is_offered_after_a_failed_verification_only_with_a_valid_record() {
    for (record, offered) in [
        (valid(), true),
        (Record::Absent, false),
        (Record::Unreadable("bad".into()), false),
    ] {
        let mut s = overridden();
        s.record = record;
        let row = model_of(&s, Some(&(Op::Set, Err(OpError::Verify))))
            .error_row
            .unwrap();
        assert_eq!(row.offers_restore, offered);
    }
}

/// A failure the record stopped is the record's band; see the test for it.
#[test]
fn other_failures_are_titled_in_the_catalogues_words_with_their_paths_and_carry_no_details() {
    let io = |reason| std::io::Error::other(reason);
    let cases = [
        (
            SetupError::Write("/a/mimeapps.list".into(), io("disk full")),
            "Couldn't save /a/mimeapps.list: disk full",
        ),
        (
            SetupError::Read("/a/mimeapps.list".into(), io("denied")),
            "Couldn't read /a/mimeapps.list: denied",
        ),
        (
            SetupError::NoRestorePoint,
            "There's no previous browser to restore",
        ),
        (
            SetupError::Unresolvable("/a/mimeapps.list".into(), io("loop")),
            "Couldn't follow /a/mimeapps.list (loop). Fix or remove that link, then try again.",
        ),
        (
            SetupError::LinkChanged {
                path: "/a/mimeapps.list".into(),
                recorded: "/d/old.list".into(),
                current: "/d/new.list".into(),
            },
            "/a/mimeapps.list now leads to /d/new.list, not /d/old.list where Signpost was set up. Point it back to restore the previous browser.",
        ),
    ];
    for (error, title) in cases {
        let m = model_of(
            &overridden(),
            Some(&(Op::Restore, Err(OpError::from(&error)))),
        );
        let row = m.error_row.unwrap();
        assert_eq!(plain(&row.title), title);
        assert_eq!(row.body, None);
        assert_eq!(row.details, Vec::<String>::new());
        assert!(!row.offers_restore, "{title}");
    }
}

/// Setup already made in another file is undone from that file, so the row offers it.
#[test]
fn a_setup_made_in_another_file_offers_restore_while_its_record_is_valid() {
    let error = OpError::from(&SetupError::PathChanged {
        recorded: "/a/mimeapps.list".into(),
        current: "/b/mimeapps.list".into(),
    });
    let last = (Op::Set, Err(error));
    let row = model_of(&overridden(), Some(&last)).error_row.unwrap();
    assert_eq!(
        plain(&row.title),
        "Signpost was set up in /a/mimeapps.list. Restore the previous browser before using Signpost in /b/mimeapps.list."
    );
    assert!(row.offers_restore);
    let mut unrecorded = overridden();
    unrecorded.record = Record::Absent;
    assert!(
        !model_of(&unrecorded, Some(&last))
            .error_row
            .unwrap()
            .offers_restore
    );
}

#[test]
fn a_task_failure_takes_its_title_from_the_catalogue_and_offers_nothing_else() {
    let m = model_of(&overridden(), Some(&(Op::Set, Err(OpError::Task))));
    let row = m.error_row.unwrap();
    assert_eq!(plain(&row.title), "The setup task failed");
    assert_eq!(row.body, None);
    assert_eq!(row.details, Vec::<String>::new());
    assert!(!row.offers_restore);
}

#[test]
fn a_successful_retry_clears_the_error_row() {
    let s = snap(Some(SIGNPOST), Some(SIGNPOST), valid());
    assert!(
        model_of(&s, Some(&(Op::Set, Err(OpError::Verify))))
            .error_row
            .is_some()
    );
    assert_eq!(model_of(&s, Some(&(Op::Set, Ok(())))).error_row, None);
    assert_eq!(model_of(&s, None).error_row, None);
}

#[test]
fn a_successful_set_toasts_with_undo_naming_signpost() {
    let s = snap(Some(SIGNPOST), Some(SIGNPOST), valid());
    let toast = model_of(&s, Some(&(Op::Set, Ok(())))).toast.unwrap();
    assert_eq!(plain(&toast.text), "Signpost now opens your web links");
    assert_eq!(toast.undo, Some(Action::Restore));
}

#[test]
fn a_successful_restore_toasts_with_what_now_opens_the_links() {
    let restored = |http: Option<&str>, https: Option<&str>| {
        let toast = model_of(
            &snap(http, https, Record::Absent),
            Some(&(Op::Restore, Ok(()))),
        )
        .toast
        .unwrap();
        assert_eq!(toast.undo, None);
        plain(&toast.text)
    };
    assert_eq!(
        restored(Some("firefox"), Some("firefox")),
        "Firefox now opens your web links"
    );
    assert_eq!(
        restored(Some("brave"), Some("firefox")),
        "HTTPS links now open in Firefox; HTTP links now open in Brave Origin"
    );
    assert_eq!(
        restored(None, Some("firefox")),
        "HTTPS links now open in Firefox; HTTP links have no default"
    );
    assert_eq!(
        restored(Some("brave"), None),
        "HTTP links now open in Brave Origin; HTTPS links have no default"
    );
    assert_eq!(restored(None, None), "Web-link defaults cleared");
}

#[test]
fn failures_and_no_operation_show_no_toast() {
    let s = snap(Some("firefox"), Some("firefox"), Record::Absent);
    let failures = [
        (Op::Set, Err(OpError::Verify)),
        (Op::Restore, Err(OpError::Failed("x".into()))),
    ];
    assert_eq!(model_of(&s, None).toast, None);
    for last in &failures {
        assert_eq!(model_of(&s, Some(last)).toast, None);
    }
}

#[test]
fn op_error_keeps_the_records_own_text_for_its_details_and_verify_carries_nothing() {
    let record = SetupError::Record("/p".into(), "bad".into());
    assert_eq!(OpError::from(&record), OpError::Record(record.to_string()));
    assert_eq!(OpError::from(&SetupError::Verify), OpError::Verify);
}
