use std::path::PathBuf;

use cosmic::iced::Color;

use super::*;
use crate::picker::Input;
use crate::profiles::{Profile, TileAction};

mod layout;

const CHROME: &str = "google-chrome.desktop";
const URI: &str = "https://example.org/";
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

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-6
}

/// Fluent wraps substituted values in direction-isolation marks.
fn plain(text: &str) -> String {
    text.replace(['\u{2068}', '\u{2069}'], "")
}

fn plain_tile(app: &str) -> Tile {
    Tile {
        app_id: app.into(),
        app_name: app.into(),
        label: app.into(),
        icon: None,
        profile: None,
        flatpak: false,
        actions: vec![],
    }
}

fn profile_tile(app: &str, key: &str, color: Option<Rgb>) -> Tile {
    Tile {
        label: key.into(),
        profile: Some(Profile {
            key: key.into(),
            label: key.into(),
            dir: PathBuf::new(),
            color,
        }),
        ..plain_tile(app)
    }
}

/// The chosen colors as the config store holds them, without a store.
fn overrides(entries: &[(&str, Rgb)]) -> Overrides {
    let map: serde_json::Map<String, serde_json::Value> = entries
        .iter()
        .map(|(key, color)| {
            (
                (*key).to_owned(),
                serde_json::to_value(color).expect("a color serializes"),
            )
        })
        .collect();
    serde_json::from_value(map.into()).expect("a map of colors")
}

fn parts(uri: &str) -> (bool, String, String) {
    let LinkParts { secure, host, rest } = link_parts(uri);
    (secure, host, rest)
}

#[test]
fn an_https_link_is_secure_and_splits_into_host_and_the_rest() {
    assert_eq!(
        parts("https://example.org/a/b?x=1#frag"),
        (true, "example.org".into(), "/a/b?x=1#frag".into())
    );
    assert_eq!(
        parts("HTTPS://Example.org/p"),
        (true, "example.org".into(), "/p".into()),
        "the parsed URL is normalized"
    );
}

#[test]
fn an_http_link_is_not_secure() {
    assert_eq!(
        parts("http://example.org/p"),
        (false, "example.org".into(), "/p".into())
    );
}

#[test]
fn the_path_is_shown_as_the_url_stores_it() {
    assert_eq!(parts("https://example.org/a b?x=%20#f").2, "/a%20b?x=%20#f");
}

#[test]
fn a_port_shows_only_when_it_is_not_the_schemes_default() {
    assert_eq!(parts("https://example.org:8443/").1, "example.org:8443");
    assert_eq!(parts("https://example.org:443/").1, "example.org");
    assert_eq!(parts("http://example.org:80/").1, "example.org");
    assert_eq!(parts("http://example.org:443/").1, "example.org:443");
    assert_eq!(parts("http://[::1]:3000/").1, "[::1]:3000");
}

#[test]
fn userinfo_is_never_shown() {
    let (_, host, rest) = parts("https://user:secret@example.org:8443/p");
    assert_eq!((host.as_str(), rest.as_str()), ("example.org:8443", "/p"));
    assert!(!format!("{host}{rest}").contains("secret"));
}

#[test]
fn a_bare_slash_is_not_a_line_of_its_own() {
    assert_eq!(parts("https://example.org").2, "");
    assert_eq!(parts("https://example.org/").2, "");
    assert_eq!(parts("https://example.org/?q=1").2, "/?q=1");
    assert_eq!(parts("https://example.org/#top").2, "/#top");
}

#[test]
fn an_international_host_shows_as_punycode() {
    assert_eq!(parts("https://bücher.example/").1, "xn--bcher-kva.example");
}

#[test]
fn text_that_is_not_a_link_is_shown_as_it_is_and_never_as_secure() {
    assert_eq!(
        parts("not a url"),
        (false, "not a url".into(), String::new())
    );
}

#[test]
fn the_next_link_row_shows_its_host_and_path() {
    assert_eq!(
        host_and_path("https://github.com/lkbddh/signpost"),
        "github.com/lkbddh/signpost"
    );
    assert_eq!(host_and_path("https://github.com/"), "github.com");
}

#[test]
fn a_tile_shows_its_override_before_the_browsers_seed() {
    let tile = profile_tile(CHROME, "Default", Some(ORANGE));
    assert_eq!(effective_color(&tile, &Overrides::default()), Some(ORANGE));
    let chosen = overrides(&[("google-chrome.desktop/Default", TEAL)]);
    assert_eq!(effective_color(&tile, &chosen), Some(TEAL));
    let seedless = profile_tile(CHROME, "Default", None);
    assert_eq!(
        effective_color(&seedless, &chosen),
        Some(TEAL),
        "a profile the browser gave no color can still be colored"
    );
    assert_eq!(effective_color(&seedless, &Overrides::default()), None);
}

#[test]
fn an_override_belongs_to_one_app_and_one_profile() {
    let chosen = overrides(&[("google-chrome.desktop/Default", TEAL)]);
    let other_profile = profile_tile(CHROME, "Profile 1", Some(ORANGE));
    assert_eq!(effective_color(&other_profile, &chosen), Some(ORANGE));
    let other_app = profile_tile("brave-browser.desktop", "Default", None);
    assert_eq!(effective_color(&other_app, &chosen), None);
}

#[test]
fn a_tile_without_a_profile_has_no_color() {
    let chosen = overrides(&[("firefox.desktop/default", TEAL)]);
    assert_eq!(
        effective_color(&plain_tile("firefox.desktop"), &chosen),
        None
    );
}

#[test]
fn a_badge_needs_two_profile_tiles_of_the_app_and_a_color() {
    let tiles = vec![
        profile_tile(CHROME, "a", Some(ORANGE)),
        profile_tile(CHROME, "b", None),
        profile_tile(CHROME, "c", Some(TEAL)),
        plain_tile("firefox.desktop"),
        profile_tile("brave-browser.desktop", "only", Some(ORANGE)),
    ];
    let none = Overrides::default();
    let badges: Vec<Option<Rgb>> = (0..tiles.len())
        .map(|index| badge_color(&tiles, index, &none))
        .collect();
    assert_eq!(badges, [Some(ORANGE), None, Some(TEAL), None, None]);
}

#[test]
fn a_badge_wears_the_chosen_color() {
    let tiles = vec![
        profile_tile(CHROME, "a", Some(ORANGE)),
        profile_tile(CHROME, "b", None),
    ];
    let chosen = overrides(&[("google-chrome.desktop/b", TEAL)]);
    assert_eq!(badge_color(&tiles, 0, &chosen), Some(ORANGE));
    assert_eq!(badge_color(&tiles, 1, &chosen), Some(TEAL));
    assert_eq!(badge_color(&tiles, 9, &chosen), None, "no such tile");
}

#[test]
fn the_tint_is_the_profile_color_or_else_the_neutral_one() {
    let neutral = Color::from_rgba(1.0, 1.0, 1.0, 0.1);
    assert_eq!(tint(None, neutral), neutral);
    let tinted = tint(Some(ORANGE), neutral);
    assert_eq!(tinted.into_rgba8()[..3], [0xFF, 0x80, 0x00]);
    assert!((tinted.a - TINT_ALPHA).abs() < 1e-6, "{tinted:?}");
}

#[test]
fn the_waiting_chip_counts_in_the_singular_and_the_plural() {
    assert_eq!(plain(&waiting_label(1)), "1 link waiting");
    assert_eq!(plain(&waiting_label(2)), "2 links waiting");
    assert_eq!(plain(&waiting_label(12)), "12 links waiting");
}

#[test]
fn the_failure_banner_names_the_tile_that_failed() {
    let failed = Target {
        app_id: CHROME.into(),
        profile_key: Some("Work".into()),
        label: "Work".into(),
    };
    assert_eq!(
        plain(&failure_title(Some(&failed))),
        "Couldn't open this link in Work"
    );
    assert_eq!(plain(&failure_title(None)), "Couldn't open this link");
}

#[test]
fn the_grid_and_the_card_space_their_sections_alike() {
    assert!(near(f32::from(GRID_GAP), SECTION_GAP));
}

#[test]
fn the_card_is_as_wide_as_its_widest_row_of_tiles() {
    let width = |tiles: usize| {
        card_width(&Picker::new(
            URI.into(),
            vec![plain_tile("firefox.desktop"); tiles],
        ))
    };
    assert!(near(width(1), MIN_CARD_WIDTH), "a header needs room");
    assert!(
        near(width(2), MIN_CARD_WIDTH),
        "two tiles are narrower than that"
    );
    let three = 3.0 * TILE_WIDTH + 2.0 * SECTION_GAP + 2.0 * CARD_PADDING;
    assert!(near(width(3), three));
    assert!(near(width(4), three + TILE_WIDTH + SECTION_GAP));
    assert!(near(width(9), width(4)), "rows hold four tiles at most");
}

/// Rows: `[0 1 2 3] [4 5]`, three profile tiles of one app, two plain tiles, another profile tile.
fn across_apps() -> Vec<Tile> {
    let mut tiles = vec![profile_tile(CHROME, "p", None); 3];
    tiles.extend([plain_tile("brave-origin"), plain_tile("firefox")]);
    tiles.push(profile_tile(CHROME, "q", None));
    tiles
}

#[test]
fn five_tiles_of_any_apps_make_the_card_four_columns_wide() {
    let four = 4.0 * TILE_WIDTH + 3.0 * SECTION_GAP + 2.0 * CARD_PADDING;
    let mut tiles = across_apps();
    tiles.truncate(5);
    assert!(near(card_width(&Picker::new(URI.into(), tiles)), four));
    assert!(near(
        card_width(&Picker::new(URI.into(), across_apps())),
        four
    ));
}

#[test]
fn the_no_apps_card_is_as_wide_as_its_illustration() {
    assert!(near(
        card_width(&Picker::new(URI.into(), Vec::new())),
        ILLUSTRATION_SIZE.width
    ));
}

#[test]
fn the_grid_leaves_the_card_and_its_bands_room_for_the_rows_around_it() {
    let mut picker = Picker::new(URI.into(), vec![plain_tile("firefox.desktop")]);
    let block = |picker: &Picker, waiting, rows: f32, bands: f32| {
        CARD_CHROME + rows + body_max_height(picker, waiting) + SHEET_STEP * bands
    };
    assert!(near(block(&picker, 0, 0.0, 0.0), MAX_HEIGHT), "header only");
    let queue = SECTION_GAP + QUEUE_ROW_HEIGHT;
    assert!(near(block(&picker, 1, queue, 1.0), MAX_HEIGHT), "one band");
    assert!(near(block(&picker, 2, queue, 2.0), MAX_HEIGHT), "two bands");
    assert!(
        near(block(&picker, 5, queue, to_f32(MAX_SHEETS)), MAX_HEIGHT),
        "no more bands than the cap"
    );
    picker.fail("exited".into(), None);
    let banner = SECTION_GAP + BANNER_HEIGHT;
    assert!(near(block(&picker, 0, banner, 0.0), MAX_HEIGHT), "banner");
    assert!(
        near(block(&picker, 1, banner + queue, 1.0), MAX_HEIGHT),
        "banner, queue row and a band"
    );
}

#[test]
fn the_no_apps_ground_yields_before_its_panel_and_both_fit_the_body() {
    let mut picker = Picker::new(URI.into(), Vec::new());
    assert_eq!(
        no_apps_heights(&picker, 0),
        (NO_APPS_CARD_HEIGHT, SECTION_GAP + NO_APPS_SCENE),
        "room enough"
    );
    for error in [false, true] {
        for details in [false, true] {
            for waiting in 0..=2 * MAX_SHEETS {
                if error {
                    picker.fail("exited".into(), None);
                }
                picker.details_open = details;
                let (panel, ground) = no_apps_heights(&picker, waiting);
                let note = format!("error {error}, details {details}, waiting {waiting}");
                assert!(ground >= 0.0 && panel <= NO_APPS_CARD_HEIGHT, "{note}");
                assert!(
                    panel + ground <= body_max_height(&picker, waiting) + 0.01,
                    "{note}"
                );
                assert!(
                    near(panel, NO_APPS_CARD_HEIGHT) || near(ground, 0.0),
                    "{note}"
                );
            }
        }
    }
}

#[test]
fn open_details_take_their_cap_out_of_the_grid() {
    let mut picker = Picker::new(URI.into(), vec![plain_tile("firefox.desktop")]);
    picker.fail("exited".into(), None);
    let collapsed = body_max_height(&picker, 0);
    picker.handle(Input::ToggleDetails);
    assert!(
        near(
            collapsed - body_max_height(&picker, 0),
            BANNER_GAP + DETAILS_MAX_HEIGHT
        ),
        "{collapsed} against {}",
        body_max_height(&picker, 0)
    );
}

#[test]
fn the_grid_keeps_a_tile_row_whatever_surrounds_it() {
    let mut picker = Picker::new(URI.into(), vec![plain_tile("firefox.desktop")]);
    for error in [false, true] {
        for details in [false, true] {
            for waiting in [0, 1, 5] {
                if error {
                    picker.fail("exited".into(), None);
                }
                picker.details_open = details;
                assert!(
                    body_max_height(&picker, waiting) >= TILE_HEIGHT,
                    "error {error}, details {details}, waiting {waiting}"
                );
            }
        }
    }
}

#[test]
fn every_menu_row_has_the_icon_the_spec_names() {
    let icons: Vec<Icon> = [
        MenuItem::Open,
        MenuItem::Action(TileAction::Private),
        MenuItem::Action(TileAction::Desktop {
            id: "new-window".into(),
            label: "New Window".into(),
        }),
        MenuItem::OpenKeep,
        MenuItem::Copy,
    ]
    .iter()
    .map(menu_icon)
    .collect();
    assert_eq!(
        icons,
        [
            Icon::ArrowSquareOut,
            Icon::Detective,
            Icon::AppWindow,
            Icon::PushPin,
            Icon::Copy
        ]
    );
}

#[test]
fn the_scroll_target_keeps_the_focused_row_in_view() {
    assert!(near(scroll_fraction(0, 1), 0.0));
    assert!(near(scroll_fraction(0, 5), 0.0));
    assert!(near(scroll_fraction(2, 5), 0.5));
    assert!(near(scroll_fraction(4, 5), 1.0));
    assert!(near(scroll_fraction(9, 5), 1.0), "never past the end");
    let mut tiles = vec![plain_tile("a"); 17];
    tiles[13].actions = vec![TileAction::Private; 3];
    let mut picker = Picker::new(URI.into(), tiles);
    picker.focus = 13;
    assert!(near(focus_scroll(&picker), 0.75), "row 3 of 5 grid rows");
    picker.handle(Input::OpenMenu(None));
    picker.handle(Input::Move(0, 1));
    assert!(
        near(focus_scroll(&picker), 1.0 / 5.0),
        "menu row 1 of Open, 3 actions, Open and keep, Copy"
    );
}

#[test]
fn the_scroll_target_follows_rows_filled_in_order() {
    // Rows: [0 1 2 3] [4 5 6 7] [8].
    let mut tiles = vec![plain_tile("firefox")];
    tiles.extend(vec![profile_tile("chrome", "p", None); 6]);
    tiles.extend([plain_tile("brave"), plain_tile("vivaldi")]);
    let mut picker = Picker::new(URI.into(), tiles);
    let rows = [0.0, 0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 0.5, 1.0];
    for (focus, row) in rows.into_iter().enumerate() {
        picker.focus = focus;
        assert!(near(focus_scroll(&picker), row), "tile {focus}");
    }
}

#[test]
fn with_equal_row_heights_the_focused_row_is_always_wholly_visible() {
    // The rows of a grid or of a menu are as tall as each other, so the snap is exact for any constant gap
    // and any viewport at least one row tall.
    for height in [TILE_HEIGHT, TILE_HEIGHT / 4.0] {
        for gap in [0.0_f32, 4.0, 8.0, 16.0] {
            for rows in 1..=12_u16 {
                let content = f32::from(rows) * height + f32::from(rows - 1) * gap;
                let mut viewport = height;
                while viewport <= content {
                    for row in 0..rows {
                        let offset = scroll_fraction(usize::from(row), usize::from(rows))
                            * (content - viewport);
                        let top = f32::from(row) * (height + gap);
                        assert!(
                            top >= offset - 0.01 && top + height <= offset + viewport + 0.01,
                            "row {row}/{rows}, height {height}, gap {gap}, viewport {viewport}"
                        );
                    }
                    viewport += 7.0;
                }
            }
        }
    }
}

/// The queue's Next button is reached with Tab, so it wears libcosmic's own focus ring while focused.
#[test]
fn a_focused_row_button_wears_the_native_focus_ring() {
    use cosmic::widget::button::Catalog as _;
    let theme = cosmic::Theme::dark();
    let theme::Button::Custom {
        active, hovered, ..
    } = parts::button_class(
        |theme| row_style(theme, None),
        |theme| row_style(theme, None),
    )
    else {
        panic!("a custom class");
    };
    let native = theme.active(true, false, &theme::Button::Standard);
    for style in [active(true, &theme), hovered(true, &theme)] {
        assert!(near(style.outline_width, native.outline_width));
        assert_eq!(style.outline_color, native.outline_color);
    }
    assert!(native.outline_width > 0.0);
    assert!(
        near(active(false, &theme).outline_width, 0.0),
        "no ring unfocused"
    );
}
