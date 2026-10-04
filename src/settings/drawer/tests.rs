use std::path::PathBuf;

use super::*;
use crate::settings::ProfileRow;
use crate::settings::tests::plain;
use crate::test_support::frosted::{fill, translucent};

mod layout;

const ORANGE: Rgb = Rgb {
    r: 0xFF,
    g: 0x80,
    b: 0x00,
};
const TEAL: Rgb = Rgb {
    r: 0x12,
    g: 0xA5,
    b: 0x94,
};
const PINK: Rgb = Rgb {
    r: 0xE9,
    g: 0x3D,
    b: 0x82,
};
const CHROME: &str = "google-chrome";
const DEFAULT_KEY: &str = "google-chrome/Default";

fn profile(key: &str, label: &str, seed: Option<Rgb>, tile: Option<usize>) -> ProfileRow {
    ProfileRow {
        key: key.to_owned(),
        label: label.to_owned(),
        dir: PathBuf::from("/data").join(key),
        seed,
        tile,
    }
}

fn chrome(profiles: Vec<ProfileRow>) -> AppRow {
    AppRow {
        id: CHROME.to_owned(),
        name: "Google Chrome".to_owned(),
        icon: None,
        flatpak: false,
        profiles,
        private: false,
        desktop_actions: Vec::new(),
    }
}

fn two_profiles() -> AppRow {
    chrome(vec![
        profile("Default", "acme", Some(ORANGE), Some(1)),
        profile("Profile 1", "travel", None, Some(2)),
    ])
}

fn store_in(dir: &tempfile::TempDir) -> ColorStore {
    ColorStore::at(dir.path().to_path_buf()).unwrap()
}

/// Overrides as a store would hand them back; only the choices matter, so the store is thrown away.
fn chosen(choices: &[(&str, Rgb)]) -> Overrides {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(&dir);
    let mut overrides = Overrides::default();
    for (key, color) in choices {
        overrides.set(&store, key, Some(*color)).unwrap();
    }
    overrides
}

fn colors_of(content: &Content<'_>) -> Vec<Option<Rgb>> {
    content.profiles.iter().map(|line| line.color).collect()
}

fn drawer_with_popover(profile: &str) -> Drawer {
    let mut drawer = Drawer::open(CHROME.to_owned());
    drawer.swatch = Some(profile.to_owned());
    drawer
}

fn popover_after(events: Vec<Swatch>) -> Option<String> {
    let mut drawer = Drawer::open(CHROME.to_owned());
    let mut overrides = Overrides::default();
    for event in events {
        drawer.update(event, None, &mut overrides);
    }
    drawer.swatch
}

#[test]
fn each_profile_shows_its_name_key_color_and_keycap() {
    let app = two_profiles();
    let shown = content(&app, &Overrides::default());
    let lines: Vec<_> = shown
        .profiles
        .iter()
        .map(|line| (line.label, line.key, line.color, line.keycap))
        .collect();
    assert_eq!(
        lines,
        [
            ("acme", "Default", Some(ORANGE), Some(1)),
            ("travel", "Profile 1", None, Some(2)),
        ]
    );
}

#[test]
fn a_chosen_color_beats_the_browsers_and_only_for_its_own_app_and_profile() {
    let app = chrome(vec![
        profile("Default", "a", Some(ORANGE), Some(1)),
        profile("Profile 1", "b", None, Some(2)),
        profile("Profile 2", "c", Some(ORANGE), Some(3)),
    ]);
    let overrides = chosen(&[
        (DEFAULT_KEY, TEAL),
        ("google-chrome/Profile 1", PINK),
        ("other-browser/Profile 2", TEAL),
    ]);
    assert_eq!(
        colors_of(&content(&app, &overrides)),
        [Some(TEAL), Some(PINK), Some(ORANGE)]
    );
}

#[test]
fn a_profile_with_no_color_at_all_shows_none() {
    let app = chrome(vec![profile("Default", "a", None, Some(1))]);
    assert_eq!(colors_of(&content(&app, &Overrides::default())), [None]);
}

#[test]
fn keycaps_stop_after_the_ninth_tile_and_a_profile_without_a_tile_has_none() {
    let tiles = [Some(1), Some(9), Some(10), Some(14), None];
    let app = chrome(
        tiles
            .iter()
            .enumerate()
            .map(|(n, tile)| profile(&format!("P{n}"), "p", None, *tile))
            .collect(),
    );
    let keycaps: Vec<Option<u8>> = content(&app, &Overrides::default())
        .profiles
        .iter()
        .map(|line| line.keycap)
        .collect();
    assert_eq!(keycaps, [Some(1), Some(9), None, None, None]);
}

#[test]
fn the_caption_names_the_desktop_id_and_whether_the_app_is_native_or_flatpak() {
    let native = content(&two_profiles(), &Overrides::default()).caption;
    assert_eq!(plain(&native), "google-chrome.desktop · Native");
    let flatpak = AppRow {
        id: "org.mozilla.firefox".to_owned(),
        flatpak: true,
        ..two_profiles()
    };
    let caption = content(&flatpak, &Overrides::default()).caption;
    assert_eq!(plain(&caption), "org.mozilla.firefox.desktop · Flatpak");
}

#[test]
fn the_picker_menu_lists_the_private_window_then_each_desktop_action() {
    let app = AppRow {
        private: true,
        desktop_actions: vec!["New Window".to_owned(), "New Private Window".to_owned()],
        ..two_profiles()
    };
    assert_eq!(
        content(&app, &Overrides::default()).menu,
        [
            ("Private window".to_owned(), "Available".to_owned()),
            ("New Window".to_owned(), "Desktop action".to_owned()),
            ("New Private Window".to_owned(), "Desktop action".to_owned()),
        ]
    );
}

#[test]
fn the_groups_appear_only_when_the_app_has_something_for_them() {
    let overrides = Overrides::default();
    let bare_app = chrome(Vec::new());
    let bare = content(&bare_app, &overrides);
    assert!(bare.profiles.is_empty() && bare.menu.is_empty());

    let no_profiles = AppRow {
        private: true,
        ..chrome(Vec::new())
    };
    let shown = content(&no_profiles, &overrides);
    assert!(shown.profiles.is_empty(), "no Profiles group");
    assert_eq!(shown.menu.len(), 1);

    let only_actions = AppRow {
        desktop_actions: vec!["Act".to_owned()],
        ..two_profiles()
    };
    let shown = content(&only_actions, &overrides);
    assert_eq!(shown.profiles.len(), 2);
    assert_eq!(shown.menu.len(), 1, "no private window row");
}

#[test]
fn choosing_a_color_saves_it_and_closes_the_popover_and_reset_removes_it() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(&dir);
    let mut overrides = Overrides::default();
    let mut drawer = Drawer::open(CHROME.to_owned());

    drawer.update(Swatch::Open("Default".into()), Some(&store), &mut overrides);
    drawer.update(Swatch::Choose(Some(TEAL)), Some(&store), &mut overrides);

    assert_eq!(overrides.get(DEFAULT_KEY), Some(TEAL), "in memory");
    assert_eq!(
        store.load().unwrap().get(DEFAULT_KEY),
        Some(TEAL),
        "on disk"
    );
    assert_eq!(
        (drawer.swatch.as_deref(), drawer.error.as_deref()),
        (None, None)
    );

    drawer.update(Swatch::Open("Default".into()), Some(&store), &mut overrides);
    drawer.update(Swatch::Choose(None), Some(&store), &mut overrides);

    assert_eq!(overrides.get(DEFAULT_KEY), None);
    assert_eq!(store.load().unwrap().get(DEFAULT_KEY), None);
    assert_eq!(drawer.swatch, None);
}

#[test]
fn a_choice_with_no_popover_open_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(&dir);
    let mut overrides = Overrides::default();
    let mut drawer = Drawer::open(CHROME.to_owned());

    drawer.update(Swatch::Choose(Some(TEAL)), Some(&store), &mut overrides);

    assert_eq!(overrides, Overrides::default());
    assert_eq!(store.load().unwrap(), Overrides::default());
}

#[test]
fn a_failing_store_keeps_the_old_color_and_the_drawer_shows_why() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(&dir);
    let mut overrides = Overrides::default();
    let mut drawer = Drawer::open(CHROME.to_owned());
    drawer.update(Swatch::Open("Default".into()), Some(&store), &mut overrides);
    drawer.update(Swatch::Choose(Some(TEAL)), Some(&store), &mut overrides);

    std::fs::remove_dir_all(dir.path()).unwrap();
    for choice in [Some(ORANGE), None] {
        drawer.update(Swatch::Open("Default".into()), Some(&store), &mut overrides);
        drawer.update(Swatch::Choose(choice), Some(&store), &mut overrides);

        assert_eq!(overrides.get(DEFAULT_KEY), Some(TEAL), "{choice:?}");
        let error = drawer.error.as_deref().expect("the failure is shown");
        assert!(error.starts_with("Couldn't save the color:"), "{error}");
        assert_eq!(drawer.swatch, None, "the popover closes either way");
    }
}

#[test]
fn a_later_success_clears_the_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut overrides = Overrides::default();
    let mut drawer = Drawer::open(CHROME.to_owned());
    drawer.update(Swatch::Open("Default".into()), None, &mut overrides);
    drawer.update(Swatch::Choose(Some(TEAL)), None, &mut overrides);
    assert!(drawer.error.is_some());

    let store = store_in(&dir);
    drawer.update(Swatch::Open("Default".into()), Some(&store), &mut overrides);
    drawer.update(Swatch::Choose(Some(TEAL)), Some(&store), &mut overrides);
    assert_eq!(drawer.error, None);
}

#[test]
fn with_no_store_a_choice_is_an_error_not_a_panic() {
    let mut overrides = Overrides::default();
    let mut drawer = Drawer::open(CHROME.to_owned());
    drawer.update(Swatch::Open("Default".into()), None, &mut overrides);
    drawer.update(Swatch::Choose(Some(TEAL)), None, &mut overrides);

    assert_eq!(overrides, Overrides::default());
    let error = drawer.error.expect("the failure is shown");
    assert!(error.starts_with("Couldn't save the color:"), "{error}");
}

#[test]
fn pressing_another_swatch_leaves_that_one_open_whichever_message_arrives_first() {
    let open_b_then_close_a = vec![
        Swatch::Open("A".into()),
        Swatch::Open("B".into()),
        Swatch::Close("A".into()),
    ];
    let close_a_then_open_b = vec![
        Swatch::Open("A".into()),
        Swatch::Close("A".into()),
        Swatch::Open("B".into()),
    ];
    assert_eq!(popover_after(open_b_then_close_a).as_deref(), Some("B"));
    assert_eq!(popover_after(close_a_then_open_b).as_deref(), Some("B"));
}

#[test]
fn a_popovers_own_close_closes_it_and_dismiss_closes_whichever_is_open() {
    let own = vec![Swatch::Open("A".into()), Swatch::Close("A".into())];
    assert_eq!(popover_after(own), None);
    let dismissed = vec![Swatch::Open("A".into()), Swatch::Dismiss];
    assert_eq!(popover_after(dismissed), None);
    assert_eq!(popover_after(vec![Swatch::Dismiss]), None);
}

#[test]
fn a_refresh_keeps_the_drawer_while_its_app_is_listed_and_closes_it_when_the_app_is_gone() {
    let other = AppRow {
        id: "firefox".to_owned(),
        ..two_profiles()
    };
    let drawer = drawer_with_popover("Profile 1");

    let kept = drawer.clone().follow(&[other.clone(), two_profiles()]);
    assert_eq!(kept, Some(drawer.clone()));
    assert_eq!(drawer.follow(&[other]), None);
}

#[test]
fn a_refresh_closes_the_popover_of_a_profile_that_is_gone_and_keeps_the_error() {
    let mut drawer = drawer_with_popover("Profile 1");
    drawer.error = Some("disk full".to_owned());
    let one_left = chrome(vec![profile("Default", "acme", None, Some(1))]);

    let followed = drawer.follow(&[one_left]).expect("the app is still listed");

    assert_eq!(followed.swatch, None);
    assert_eq!(followed.error.as_deref(), Some("disk full"));
}

#[test]
fn the_color_popover_stays_opaque_in_a_translucent_window() {
    for dark in [true, false] {
        let theme = translucent(dark, 0.1);
        let popover = fill(&theme, &theme::Container::Dialog(true));
        assert!((popover.a - 1.0).abs() < 1e-6, "dark={dark}: {popover:?}");
        assert_eq!(
            popup_surface(&theme),
            popover,
            "the swatches' outlines are checked against the popover's own surface"
        );
    }
}
