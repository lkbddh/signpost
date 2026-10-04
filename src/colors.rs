use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use cosmic::cosmic_config::{self, ConfigGet, ConfigSet};

use crate::bus::APP_ID;
use crate::profiles::Rgb;

const CONFIG_VERSION: u64 = 1;
const PROFILE_COLORS_KEY: &str = "profile_colors";
/// WCAG 2.x minimum contrast for non-text UI components.
const MIN_COMPONENT_CONTRAST: f64 = 3.0;

const fn rgb(r: u8, g: u8, b: u8) -> Rgb {
    Rgb { r, g, b }
}

/// Four rows of eight: pastels, vivid, deep, then neutrals.
pub const PALETTE: [Rgb; 32] = [
    rgb(0xFF, 0xB3, 0xB3),
    rgb(0xFF, 0xD0, 0xA8),
    rgb(0xFF, 0xF0, 0xA0),
    rgb(0xB8, 0xF0, 0xB8),
    rgb(0xA8, 0xED, 0xE6),
    rgb(0xB3, 0xD4, 0xFF),
    rgb(0xD6, 0xC2, 0xFF),
    rgb(0xFF, 0xC2, 0xE6),
    rgb(0xE5, 0x48, 0x4D),
    rgb(0xFF, 0x80, 0x00),
    rgb(0xF5, 0xD9, 0x0A),
    rgb(0x30, 0xA4, 0x6C),
    rgb(0x12, 0xA5, 0x94),
    rgb(0x00, 0x91, 0xFF),
    rgb(0x8E, 0x4E, 0xC6),
    rgb(0xE9, 0x3D, 0x82),
    rgb(0xA7, 0x2B, 0x2F),
    rgb(0xB8, 0x5A, 0x00),
    rgb(0xA8, 0x8F, 0x00),
    rgb(0x1E, 0x70, 0x46),
    rgb(0x0B, 0x6E, 0x62),
    rgb(0x00, 0x59, 0xA8),
    rgb(0x5C, 0x2E, 0x8C),
    rgb(0xA3, 0x28, 0x5A),
    rgb(0xD4, 0xB9, 0x96),
    rgb(0xAD, 0x7F, 0x58),
    rgb(0x7A, 0x5A, 0x3E),
    rgb(0xE0, 0xE0, 0xE0),
    rgb(0xA6, 0xA6, 0xA6),
    rgb(0x73, 0x73, 0x73),
    rgb(0x4A, 0x4A, 0x4A),
    rgb(0x26, 0x26, 0x26),
];

#[must_use]
pub fn override_key(desktop_id: &str, profile_key: &str) -> String {
    format!("{desktop_id}/{profile_key}")
}

/// The user's choice wins over the browser's own seed.
#[must_use]
pub fn effective(seed: Option<Rgb>, chosen: Option<Rgb>) -> Option<Rgb> {
    chosen.or(seed)
}

/// Whether a dot needs a contrasting outline to stay visible on `background`.
#[must_use]
pub fn needs_outline(dot: Rgb, background: Rgb) -> bool {
    contrast_ratio(luminance(dot), luminance(background)) < MIN_COMPONENT_CONTRAST
}

/// WCAG 2.x relative luminance of an sRGB color.
fn luminance(color: Rgb) -> f64 {
    let linear = |channel: u8| {
        let c = f64::from(channel) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
}

fn contrast_ratio(a: f64, b: f64) -> f64 {
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// User color choices, keyed by [`override_key`]; serialized as a plain map.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Overrides(BTreeMap<String, Rgb>);

impl Overrides {
    #[must_use]
    pub fn get(&self, key: &str) -> Option<Rgb> {
        self.0.get(key).copied()
    }

    /// Choose `color` for `key`, or reset it with `None`. The store is written first, so a failed write
    /// leaves `self` as it was.
    ///
    /// # Errors
    /// The store's write error.
    pub fn set(
        &mut self,
        store: &ColorStore,
        key: &str,
        color: Option<Rgb>,
    ) -> Result<(), cosmic_config::Error> {
        let mut next = self.clone();
        match color {
            Some(color) => next.0.insert(key.to_owned(), color),
            None => next.0.remove(key),
        };
        store.0.set(PROFILE_COLORS_KEY, &next)?;
        *self = next;
        Ok(())
    }
}

pub struct ColorStore(cosmic_config::Config);

impl ColorStore {
    /// # Errors
    /// No config directory, or it cannot be created.
    pub fn open() -> Result<Self, cosmic_config::Error> {
        cosmic_config::Config::new(APP_ID, CONFIG_VERSION).map(Self)
    }

    /// A store rooted at `dir` instead of the user's config directory.
    ///
    /// # Errors
    /// The directory cannot be created.
    pub fn at(dir: PathBuf) -> Result<Self, cosmic_config::Error> {
        cosmic_config::Config::with_custom_path(APP_ID, CONFIG_VERSION, dir).map(Self)
    }

    /// A store that was never written loads empty.
    ///
    /// # Errors
    /// The stored value cannot be read or parsed.
    pub fn load(&self) -> Result<Overrides, cosmic_config::Error> {
        // `get` falls back to a system default and reports `NoConfigDirectory` when there is none.
        match self.0.get_local(PROFILE_COLORS_KEY) {
            Err(cosmic_config::Error::NotFound) => Ok(Overrides::default()),
            loaded => loaded,
        }
    }
}

/// How long startup waits for the saved colours.
const LOAD_TIMEOUT: Duration = Duration::from_secs(2);

/// The colours `store` holds, with the store to save new choices in. A store that cannot be read is not handed
/// back: a choice made now would write over every colour it could not read.
#[must_use]
pub fn loaded(store: ColorStore) -> (Option<ColorStore>, Overrides) {
    loaded_within(store, LOAD_TIMEOUT, ColorStore::load)
}

/// [`loaded`] with `load` on a thread of its own, waited for at most `timeout`: cosmic-config checks the key is a
/// regular file and then reads it by name, so a pipe swapped in between would block that read. A store still
/// reading is never handed back.
fn loaded_within(
    store: ColorStore,
    timeout: Duration,
    load: impl FnOnce(&ColorStore) -> Result<Overrides, cosmic_config::Error> + Send + 'static,
) -> (Option<ColorStore>, Overrides) {
    let (tx, rx) = std::sync::mpsc::channel();
    let reading = std::thread::Builder::new()
        .name("profile-colors".into())
        .spawn(move || {
            let result = load(&store);
            let _ = tx.send((store, result));
        });
    let failure = match reading.map(|_| rx.recv_timeout(timeout)) {
        Ok(Ok((store, Ok(overrides)))) => return (Some(store), overrides),
        Ok(Ok((_, Err(error)))) => error.to_string(),
        Ok(Err(_)) => "the read did not end in time".to_owned(),
        Err(error) => error.to_string(),
    };
    tracing::warn!(%failure, "saved profile colors cannot be read; showing the browsers' own and saving none");
    (None, Overrides::default())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    const WHITE: Rgb = Rgb {
        r: 0xFF,
        g: 0xFF,
        b: 0xFF,
    };
    const BLACK: Rgb = Rgb { r: 0, g: 0, b: 0 };
    const WINDOW_DARK: Rgb = Rgb {
        r: 0x14,
        g: 0x15,
        b: 0x15,
    };
    const KEY: &str = "org.mozilla.firefox.desktop/Work";

    fn store_in(dir: &tempfile::TempDir) -> ColorStore {
        ColorStore::at(dir.path().to_path_buf()).unwrap()
    }

    fn key_file(dir: &tempfile::TempDir) -> PathBuf {
        dir.path()
            .join("cosmic")
            .join(APP_ID)
            .join("v1")
            .join(PROFILE_COLORS_KEY)
    }

    #[test]
    fn palette_keeps_the_spec_order() {
        let hex = |c: Rgb| format!("{:02X}{:02X}{:02X}", c.r, c.g, c.b);
        assert_eq!(hex(PALETTE[0]), "FFB3B3");
        assert_eq!(hex(PALETTE[9]), "FF8000");
        assert_eq!(hex(PALETTE[31]), "262626");
    }

    #[test]
    fn palette_colors_are_distinct() {
        for (i, color) in PALETTE.iter().enumerate() {
            assert!(!PALETTE[i + 1..].contains(color), "{color:?} repeats");
        }
    }

    #[test]
    fn override_key_joins_desktop_id_and_profile_key() {
        assert_eq!(
            override_key("google-chrome.desktop", "Profile 1"),
            "google-chrome.desktop/Profile 1"
        );
    }

    #[test]
    fn chosen_color_beats_the_seed() {
        assert_eq!(effective(Some(ORANGE), Some(TEAL)), Some(TEAL));
        assert_eq!(effective(None, Some(TEAL)), Some(TEAL));
    }

    #[test]
    fn seed_is_the_fallback() {
        assert_eq!(effective(Some(ORANGE), None), Some(ORANGE));
        assert_eq!(effective(None, None), None);
    }

    #[test]
    fn set_then_reset_in_memory() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(&dir);
        let mut overrides = Overrides::default();
        assert_eq!(overrides.get(KEY), None);
        overrides.set(&store, KEY, Some(TEAL)).unwrap();
        assert_eq!(overrides.get(KEY), Some(TEAL));
        overrides.set(&store, KEY, Some(ORANGE)).unwrap();
        assert_eq!(overrides.get(KEY), Some(ORANGE));
        overrides.set(&store, KEY, None).unwrap();
        assert_eq!(overrides.get(KEY), None);
    }

    #[test]
    fn empty_store_loads_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(store_in(&dir).load().unwrap(), Overrides::default());
    }

    #[test]
    fn unreadable_stored_value_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(&dir);
        std::fs::write(key_file(&dir), "not a map").unwrap();
        assert!(store.load().is_err());
    }

    #[test]
    fn saved_overrides_load_from_a_second_store() {
        let dir = tempfile::tempdir().unwrap();
        let mut overrides = Overrides::default();
        overrides.set(&store_in(&dir), KEY, Some(TEAL)).unwrap();
        overrides
            .set(
                &store_in(&dir),
                "google-chrome.desktop/Default",
                Some(ORANGE),
            )
            .unwrap();
        overrides.set(&store_in(&dir), KEY, None).unwrap();

        let stored = std::fs::read_to_string(key_file(&dir)).unwrap();
        assert!(stored.starts_with('{'), "stored as a plain map: {stored}");
        let loaded = store_in(&dir).load().unwrap();
        assert_eq!(loaded, overrides);
        assert_eq!(loaded.get("google-chrome.desktop/Default"), Some(ORANGE));
        assert_eq!(loaded.get(KEY), None);
    }

    #[test]
    fn write_failure_keeps_the_old_overrides() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(&dir);
        let mut overrides = Overrides::default();
        overrides.set(&store, KEY, Some(TEAL)).unwrap();

        let key_file = key_file(&dir);
        std::fs::remove_file(&key_file).unwrap();
        std::fs::create_dir(&key_file).unwrap();

        assert!(overrides.set(&store, KEY, Some(ORANGE)).is_err());
        assert!(overrides.set(&store, KEY, None).is_err());
        assert_eq!(overrides.get(KEY), Some(TEAL));
    }

    #[test]
    fn outline_when_the_dot_vanishes_into_the_background() {
        assert!(needs_outline(WHITE, WHITE));
        assert!(needs_outline(BLACK, WINDOW_DARK));
        assert!(!needs_outline(ORANGE, WINDOW_DARK));
    }

    #[test]
    fn an_unreadable_store_is_not_handed_back_so_nothing_writes_over_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(&dir);
        std::fs::write(key_file(&dir), "not a map").unwrap();
        let (kept, overrides) = loaded(store);
        assert!(
            kept.is_none(),
            "a store that cannot be read is not kept for writing"
        );
        assert_eq!(overrides, Overrides::default());
        assert_eq!(
            std::fs::read_to_string(key_file(&dir)).unwrap(),
            "not a map"
        );
    }

    /// cosmic-config checks the key is a regular file, then reads it by name: a pipe swapped in between blocks that
    /// read, and startup waits for it only so long, then goes on without the saved colors.
    #[test]
    fn a_read_that_never_ends_leaves_the_colors_unsaved_instead_of_holding_startup() {
        let dir = tempfile::tempdir().unwrap();
        let pipe = dir.path().join("swapped");
        crate::test_support::pipe_at(&pipe);
        let started = std::time::Instant::now();
        let (store, overrides) = loaded_within(
            store_in(&dir),
            std::time::Duration::from_millis(100),
            move |_| {
                let _ = std::fs::read_to_string(&pipe);
                Ok(Overrides::default())
            },
        );
        assert!(store.is_none(), "a store still reading is never written");
        assert_eq!(overrides, Overrides::default());
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }
}
