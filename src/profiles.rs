use std::path::{Path, PathBuf};

use crate::index::{AppEntry, FsView};
use crate::launch::exec::{
    Launcher, insert_args, launcher, program_name, takes_url, validate_codes,
};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProfileError {
    #[error("profile store is not valid JSON: {0}")]
    Json(String),
    #[error("profile store has no profile.info_cache")]
    NoInfoCache,
    #[error("profile store unreadable: {0}")]
    Io(String),
    #[error("profile identity is ambiguous: {0}")]
    Ambiguous(String),
    #[error("profile store lists no profiles: {0}")]
    NoProfiles(String),
}

/// An opaque sRGB color, independent of any UI toolkit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChromiumProfile {
    pub dir: String,
    pub name: String,
    /// The browser's `profile_color_seed`; `None` when it is missing or not an opaque ARGB integer.
    pub color: Option<Rgb>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirefoxProfile {
    pub name: String,
    pub path: PathBuf,
}

/// Minimal INI: `[section]` headers and `key=value` lines; `;`/`#` comments and junk lines ignored.
pub fn parse_ini(text: &str) -> Vec<(String, Vec<(String, String)>)> {
    let mut out: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            out.push((name.to_owned(), Vec::new()));
        } else if let (Some((k, v)), Some(section)) = (line.split_once('='), out.last_mut()) {
            section.1.push((k.trim().to_owned(), v.trim().to_owned()));
        }
    }
    out
}

const OPAQUE: u8 = 0xFF;

/// Chromium stores `AARRGGBB` as a signed 32-bit JSON integer (`-1` is white); only opaque colors count.
fn decode_seed(seed: &serde_json::Value) -> Option<Rgb> {
    let argb = i32::try_from(seed.as_i64()?).ok()?.cast_unsigned();
    let [alpha, r, g, b] = argb.to_be_bytes();
    (alpha == OPAQUE).then_some(Rgb { r, g, b })
}

/// Names from `profile.info_cache`; order from `profile.profiles_order` only when it is complete, has no
/// duplicates and no unknown keys — otherwise by name, directory as tie-breaker.
///
/// # Errors
/// [`ProfileError::Json`] when the text is not JSON, [`ProfileError::NoInfoCache`] when `profile.info_cache` is
/// missing or not an object.
pub fn chromium_profiles(local_state: &str) -> Result<Vec<ChromiumProfile>, ProfileError> {
    let json: serde_json::Value =
        serde_json::from_str(local_state).map_err(|e| ProfileError::Json(e.to_string()))?;
    let cache = json
        .pointer("/profile/info_cache")
        .and_then(serde_json::Value::as_object)
        .ok_or(ProfileError::NoInfoCache)?;
    let mut profiles: Vec<ChromiumProfile> = cache
        .iter()
        .map(|(dir, v)| ChromiumProfile {
            dir: dir.clone(),
            name: v
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(dir)
                .to_owned(),
            color: v.get("profile_color_seed").and_then(decode_seed),
        })
        .collect();
    let order: Option<Vec<&str>> = json
        .pointer("/profile/profiles_order")
        .and_then(serde_json::Value::as_array)
        .and_then(|a| a.iter().map(serde_json::Value::as_str).collect());
    let valid = order.as_ref().is_some_and(|o| {
        let unique: std::collections::BTreeSet<&str> = o.iter().copied().collect();
        unique.len() == o.len()
            && o.len() == cache.len()
            && o.iter().all(|d| cache.contains_key(*d))
    });
    if valid {
        let order = order.unwrap_or_default();
        profiles.sort_by_key(|p| order.iter().position(|d| *d == p.dir));
    } else {
        profiles.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.dir.cmp(&b.dir)));
    }
    Ok(profiles)
}

/// `[Profile*]` sections with Name and Path, in file order; `IsRelative=1` paths resolve against `ini_dir`.
#[must_use]
pub fn firefox_profiles(profiles_ini: &str, ini_dir: &Path) -> Vec<FirefoxProfile> {
    parse_ini(profiles_ini)
        .into_iter()
        .filter(|(name, _)| name.starts_with("Profile"))
        .filter_map(|(_, keys)| {
            let get = |k: &str| {
                keys.iter()
                    .find(|(key, _)| key == k)
                    .map(|(_, v)| v.as_str())
            };
            let name = get("Name")?.to_owned();
            let raw = get("Path")?;
            let path = if get("IsRelative") == Some("1") {
                ini_dir.join(raw)
            } else {
                PathBuf::from(raw)
            };
            Some(FirefoxProfile { name, path })
        })
        .collect()
}

#[derive(Debug)]
pub enum Store {
    /// `<config root>/<dir>/Local State`
    Chromium { dir: &'static str },
    /// `profiles.ini` under the XDG root (`<config root>/<xdg>`) or the classic root (`<home root>/<classic>`).
    Firefox {
        xdg: &'static str,
        classic: &'static str,
    },
}

#[derive(Debug)]
pub struct Adapter {
    /// The flag that opens a private window.
    pub private: &'static str,
    /// Desktop ids (native and Flatpak); a Flatpak launcher must run one of these.
    pub ids: &'static [&'static str],
    /// Native executable basenames this adapter knows; anything else gets a plain tile.
    pub executables: &'static [&'static str],
    pub store: Store,
}

pub static ADAPTERS: &[Adapter] = &[
    Adapter {
        private: "--incognito",
        ids: &["google-chrome", "com.google.Chrome"],
        // Beta/unstable keep their data in `google-chrome-beta`/`-unstable`, not this store.
        executables: &["google-chrome", "google-chrome-stable"],
        store: Store::Chromium {
            dir: "google-chrome",
        },
    },
    Adapter {
        private: "--incognito",
        ids: &["chromium", "org.chromium.Chromium"],
        executables: &["chromium", "chromium-browser"],
        store: Store::Chromium { dir: "chromium" },
    },
    Adapter {
        private: "--incognito",
        ids: &["brave-browser", "com.brave.Browser"],
        executables: &["brave-browser", "brave-browser-stable", "brave"],
        store: Store::Chromium {
            dir: "BraveSoftware/Brave-Browser",
        },
    },
    Adapter {
        private: "--incognito",
        ids: &["brave-origin", "com.brave.Origin"],
        executables: &["brave-origin", "brave-origin-stable"],
        store: Store::Chromium {
            dir: "BraveSoftware/Brave-Origin",
        },
    },
    Adapter {
        private: "--incognito",
        ids: &["vivaldi-stable", "com.vivaldi.Vivaldi"],
        executables: &["vivaldi", "vivaldi-stable"],
        store: Store::Chromium { dir: "vivaldi" },
    },
    Adapter {
        private: "--incognito",
        ids: &["helium", "net.imput.helium"],
        executables: &["helium"],
        store: Store::Chromium {
            dir: "net.imput.helium",
        },
    },
    Adapter {
        private: "--inprivate",
        ids: &["microsoft-edge", "com.microsoft.Edge"],
        executables: &["microsoft-edge", "microsoft-edge-stable"],
        store: Store::Chromium {
            dir: "microsoft-edge",
        },
    },
    Adapter {
        private: "--private-window",
        ids: &["firefox", "org.mozilla.firefox"],
        executables: &["firefox", "firefox-esr", "firefox-bin"],
        store: Store::Firefox {
            xdg: "mozilla/firefox",
            classic: ".mozilla/firefox",
        },
    },
    Adapter {
        private: "--private-window",
        ids: &["librewolf", "io.gitlab.librewolf-community"],
        executables: &["librewolf"],
        store: Store::Firefox {
            xdg: "librewolf/librewolf",
            classic: ".librewolf",
        },
    },
    Adapter {
        private: "--private-window",
        ids: &["zen", "app.zen_browser.zen"],
        executables: &["zen", "zen-browser"],
        store: Store::Firefox {
            xdg: "zen",
            classic: ".zen",
        },
    },
    Adapter {
        private: "--private-window",
        ids: &["floorp", "one.ablaze.floorp"],
        executables: &["floorp"],
        store: Store::Firefox {
            xdg: "floorp",
            classic: ".floorp",
        },
    },
];

pub struct Env<'a> {
    pub home: &'a Path,
    pub config_home: &'a Path,
    /// Where the files these paths name are read: as given natively, the host's through a Flatpak sandbox.
    pub view: &'a dyn FsView,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    /// Chromium profile directory name, or the Firefox profile name.
    pub key: String,
    pub label: String,
    pub dir: PathBuf,
    /// `None` for Firefox, which has no profile color in `profiles.ini`.
    pub color: Option<Rgb>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TileAction {
    Desktop { id: String, label: String },
    Private,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tile {
    pub app_id: String,
    pub app_name: String,
    pub label: String,
    pub icon: Option<String>,
    pub profile: Option<Profile>,
    pub flatpak: bool,
    pub actions: Vec<TileAction>,
}

#[must_use]
pub fn adapter_for(desktop_id: &str) -> Option<&'static Adapter> {
    let id = desktop_id.strip_suffix(".desktop").unwrap_or(desktop_id);
    ADAPTERS.iter().find(|a| a.ids.contains(&id))
}

impl Store {
    /// The arguments that pick a profile in the browsers of this store: an Exec carrying one selects its own.
    #[must_use]
    pub fn selectors(&self) -> &'static [&'static str] {
        match self {
            Self::Chromium { .. } => &["--profile-directory"],
            Self::Firefox { .. } => &[
                "-P",
                "-p",
                "--profile",
                "-profile",
                "--ProfileManager",
                "-ProfileManager",
            ],
        }
    }

    /// The arguments that open `profile`.
    #[must_use]
    pub fn profile_args(&self, profile: &Profile) -> Vec<String> {
        match self {
            Self::Chromium { .. } => vec![format!("--profile-directory={}", profile.key)],
            Self::Firefox { .. } => vec!["-P".to_owned(), profile.key.clone()],
        }
    }
}

/// A known native executable for this adapter, or `flatpak run` of one of its ids.
#[must_use]
pub fn supports(adapter: &Adapter, launcher: &Launcher, exec: &[String]) -> bool {
    match launcher {
        Launcher::Native => exec
            .first()
            .is_some_and(|p| adapter.executables.contains(&program_name(p))),
        Launcher::Flatpak { app_id } => adapter.ids.contains(&app_id.as_str()),
    }
}

/// Chromium `--user-data-dir=<dir>` or `--user-data-dir <dir>` in Exec selects the profile root.
#[must_use]
pub fn user_data_dir(exec: &[String]) -> Option<PathBuf> {
    exec.iter().enumerate().find_map(|(i, t)| {
        t.strip_prefix("--user-data-dir=")
            .map(PathBuf::from)
            .or_else(|| {
                if t == "--user-data-dir" {
                    exec.get(i + 1).map(PathBuf::from)
                } else {
                    None
                }
            })
    })
}

/// `(config root, home root)`: the XDG config home and `$HOME`, or the Flatpak app's `config` dir and data dir.
fn roots(launcher: &Launcher, env: &Env) -> (PathBuf, PathBuf) {
    match launcher {
        Launcher::Native => (env.config_home.to_path_buf(), env.home.to_path_buf()),
        Launcher::Flatpak { app_id } => {
            let base = env.home.join(".var/app").join(app_id);
            (base.join("config"), base)
        }
    }
}

/// The text of the file at `path`, read through `view`.
fn read(view: &dyn FsView, path: &Path) -> Result<String, ProfileError> {
    let read_path = view
        .read_path(path)
        .ok_or_else(|| ProfileError::Io(format!("{}: out of reach", path.display())))?;
    crate::index::read_regular(&read_path)
        .map_err(|e| ProfileError::Io(format!("{}: {e}", path.display())))
}

/// Whether `path` is a file, looked at through `view`.
fn is_file(view: &dyn FsView, path: &Path) -> bool {
    view.read_path(path).is_some_and(|read| read.is_file())
}

/// Native launcher → `env.config_home`/`env.home` roots; Flatpak → `~/.var/app/<app_id>/{config,}` roots;
/// Chromium `--user-data-dir` in `exec` overrides the root. Firefox: if both the XDG and the classic root
/// hold a profiles.ini, identity is ambiguous.
///
/// # Errors
/// [`ProfileError::Io`] when the store file cannot be read or (Firefox) neither root has one,
/// [`ProfileError::NoProfiles`] for a Firefox store listing no profile, [`ProfileError::Ambiguous`] when
/// both Firefox roots hold a store or the data root
/// cannot be resolved here (a relative `--user-data-dir`, one containing a `%`, or any override under
/// Flatpak) or two profiles share a launch key (their launch arguments would be identical), otherwise the
/// errors of [`chromium_profiles`].
pub fn discover(
    adapter: &Adapter,
    launcher: &Launcher,
    env: &Env,
    exec: &[String],
) -> Result<Vec<Profile>, ProfileError> {
    let profiles = stored_profiles(&store_root(adapter, launcher, env, exec)?, env)?;
    // The browser sees every profile in its store, those out of this view's reach included.
    let keys: std::collections::BTreeSet<&str> = profiles.iter().map(|p| p.key.as_str()).collect();
    if keys.len() != profiles.len() {
        return Err(ProfileError::Ambiguous(
            "two profiles share a launch name".to_owned(),
        ));
    }
    // A profile whose folder is gone, or out of this view's reach, would make a tile that always fails.
    Ok(profiles
        .into_iter()
        .filter(|profile| {
            let reachable = env
                .view
                .read_path(&profile.dir)
                .is_some_and(|dir| dir.is_dir());
            if !reachable {
                tracing::debug!(dir = %profile.dir.display(), "a profile out of reach is left out");
            }
            reachable
        })
        .collect())
}

/// Where a profile store is looked for.
enum StoreRoot {
    /// The Chromium user data folder.
    Chromium(PathBuf),
    /// The Firefox folders, one of which holds `profiles.ini`.
    Firefox { xdg: PathBuf, classic: PathBuf },
}

impl StoreRoot {
    fn folders(&self) -> Vec<PathBuf> {
        match self {
            Self::Chromium(root) => vec![root.clone()],
            Self::Firefox { xdg, classic } => vec![xdg.clone(), classic.clone()],
        }
    }
}

/// Where the store of `adapter` is looked for when `launcher` runs `exec`.
///
/// # Errors
/// [`ProfileError::Ambiguous`] for a `--user-data-dir` that cannot be resolved here.
fn store_root(
    adapter: &Adapter,
    launcher: &Launcher,
    env: &Env,
    exec: &[String],
) -> Result<StoreRoot, ProfileError> {
    let (config_root, home_root) = roots(launcher, env);
    Ok(match adapter.store {
        Store::Chromium { dir } => StoreRoot::Chromium(match user_data_dir(exec) {
            // A relative root resolves against the launch cwd and a Flatpak one is a sandbox path:
            // neither can be checked from the daemon. A `%` makes the Exec token a field-code template,
            // not the literal path read here.
            Some(r)
                if !r.is_absolute()
                    || matches!(launcher, Launcher::Flatpak { .. })
                    || r.to_string_lossy().contains('%') =>
            {
                return Err(ProfileError::Ambiguous(format!(
                    "--user-data-dir {} cannot be resolved",
                    r.display()
                )));
            }
            Some(r) => r,
            None => config_root.join(dir),
        }),
        Store::Firefox { xdg, classic } => StoreRoot::Firefox {
            xdg: config_root.join(xdg),
            classic: home_root.join(classic),
        },
    })
}

/// Every profile the store at `root` lists, those out of the view's reach included.
///
/// # Errors
/// [`ProfileError::Io`] when the store file cannot be read or (Firefox) neither folder has one,
/// [`ProfileError::NoProfiles`] for a Firefox store listing no profile, [`ProfileError::Ambiguous`] when both Firefox
/// folders hold a store, otherwise the errors of [`chromium_profiles`].
fn stored_profiles(root: &StoreRoot, env: &Env) -> Result<Vec<Profile>, ProfileError> {
    match root {
        StoreRoot::Chromium(root) => Ok(chromium_profiles(&read(
            env.view,
            &root.join("Local State"),
        )?)?
        .into_iter()
        .map(|p| Profile {
            dir: root.join(&p.dir),
            key: p.dir,
            label: p.name,
            color: p.color,
        })
        .collect()),
        StoreRoot::Firefox { xdg, classic } => {
            let ini = |r: &Path| r.join("profiles.ini");
            let root = match (
                is_file(env.view, &ini(xdg)),
                is_file(env.view, &ini(classic)),
            ) {
                (true, false) => xdg,
                (false, true) => classic,
                (true, true) => {
                    return Err(ProfileError::Ambiguous(format!(
                        "profiles.ini in both {} and {}",
                        xdg.display(),
                        classic.display()
                    )));
                }
                (false, false) => {
                    return Err(ProfileError::Io(format!(
                        "no profiles.ini in {} or {}",
                        xdg.display(),
                        classic.display()
                    )));
                }
            };
            let profiles = firefox_profiles(&read(env.view, &ini(root))?, root);
            if profiles.is_empty() {
                return Err(ProfileError::NoProfiles(ini(root).display().to_string()));
            }
            Ok(profiles
                .into_iter()
                .map(|p| Profile {
                    key: p.name.clone(),
                    label: p.name,
                    dir: p.path,
                    color: None,
                })
                .collect())
        }
    }
}

/// What profile discovery reads for an app.
#[derive(Debug)]
pub struct StorePaths {
    /// The folders its store is looked for in.
    pub folders: Vec<PathBuf>,
    /// The folder of every profile the store lists, or why the store cannot be used.
    pub profiles: Result<Vec<PathBuf>, ProfileError>,
}

/// The folders profile discovery reads for `entry`, out of reach or not; `None` when it discovers no profiles for it.
#[must_use]
pub fn store_paths(entry: &AppEntry, env: &Env) -> Option<StorePaths> {
    let (adapter, launch, exec) = discoverable(entry)?;
    Some(match store_root(adapter, &launch, env, exec) {
        Err(e) => StorePaths {
            folders: Vec::new(),
            profiles: Err(e),
        },
        Ok(root) => StorePaths {
            folders: root.folders(),
            profiles: stored_profiles(&root, env)
                .map(|profiles| profiles.into_iter().map(|p| p.dir).collect()),
        },
    })
}

/// The profile still exists (its key is unique, with the same directory) and its directory is present.
#[must_use]
pub fn revalidate(
    adapter: &Adapter,
    launcher: &Launcher,
    env: &Env,
    exec: &[String],
    profile: &Profile,
) -> bool {
    env.view
        .read_path(&profile.dir)
        .is_some_and(|dir| dir.is_dir())
        && discover(adapter, launcher, env, exec).is_ok_and(|ps| {
            let mut same_key = ps.iter().filter(|p| p.key == profile.key);
            same_key.next().is_some_and(|p| p.dir == profile.dir) && same_key.next().is_none()
        })
}

/// `action` runs the adapter's browser the way `main` does (same launcher, a known program) against the same
/// data root: a native action must not reach a Flatpak store. An action without `--user-data-dir` inherits the
/// main root; one that sets its own must set the main one.
pub(crate) fn same_store(
    adapter: &Adapter,
    launch: &Launcher,
    main: &[String],
    action: &[String],
) -> bool {
    launcher(action).is_ok_and(|l| l == *launch)
        && supports(adapter, launch, action)
        && user_data_dir(action).is_none_or(|r| Some(r) == user_data_dir(main))
}

/// The profiles of `entry` under the guards [`tiles_for`] applies (a matching adapter, a supported launcher,
/// an Exec that selects no profile): all of them, also a single one. Empty when a guard fails or the store is
/// unusable.
#[must_use]
pub fn profiles_for(entry: &AppEntry, env: &Env) -> Vec<Profile> {
    let Some((adapter, launch, exec)) = discoverable(entry) else {
        return Vec::new();
    };
    discover(adapter, &launch, env, exec).unwrap_or_else(|e| {
        tracing::warn!(app = %entry.id, error = %e, "profile store unusable; showing a plain tile");
        Vec::new()
    })
}

/// The adapter, launcher and Exec `entry`'s profiles are discovered with: an adapter matches and the Exec carries
/// the link ([`link_adapter`]), the launcher is supported, and the Exec selects no profile itself.
fn discoverable(entry: &AppEntry) -> Option<(&'static Adapter, Launcher, &[String])> {
    let exec = entry.exec.as_deref().unwrap_or_default();
    let (Some(adapter), Ok(launch)) = (link_adapter(entry), launcher(exec)) else {
        return None;
    };
    (supports(adapter, &launch, exec) && insert_args(exec, &[], adapter.store.selectors()).is_ok())
        .then_some((adapter, launch, exec))
}

/// `entry`'s adapter, while its Exec carries the link: profile and private windows run that Exec, so an app whose
/// Exec drops it, taking links through its D-Bus `Open` method instead, gets neither.
fn link_adapter(entry: &AppEntry) -> Option<&'static Adapter> {
    adapter_for(&entry.id).filter(|_| entry.exec.as_deref().is_some_and(takes_url))
}

/// One tile per profile when an adapter matches, the launcher is supported, the Exec selects no profile and
/// ≥2 profiles are discovered; otherwise one plain tile. Actions: desktop actions taking a URL, plus
/// `Private` when the adapter's private flag can be inserted.
#[must_use]
pub fn tiles_for(entry: &AppEntry, env: &Env) -> Vec<Tile> {
    shown_icons(tiles_from(entry, &profiles_for(entry, env)), env.view)
}

/// An app's `Icon=` as the picker and the settings show it: a theme name as it is, and an absolute path where `view`
/// reads the file; `None` where it cannot. Only for showing: the icon field code passes the entry's own value.
#[must_use]
pub fn shown_icon(icon: Option<&str>, view: &dyn FsView) -> Option<String> {
    let icon = icon?;
    if !Path::new(icon).is_absolute() {
        return Some(icon.to_owned());
    }
    view.read_path(Path::new(icon))
        .map(|read| read.to_string_lossy().into_owned())
}

/// `tiles` with their icons as [`shown_icon`] shows them.
fn shown_icons(mut tiles: Vec<Tile>, view: &dyn FsView) -> Vec<Tile> {
    for tile in &mut tiles {
        tile.icon = shown_icon(tile.icon.as_deref(), view);
    }
    tiles
}

/// [`tiles_for`] for the `profiles` that [`profiles_for`] discovered for `entry`.
fn tiles_from(entry: &AppEntry, profiles: &[Profile]) -> Vec<Tile> {
    let adapter = link_adapter(entry);
    let exec = entry.exec.as_deref().unwrap_or_default();
    let launch = launcher(exec).ok();
    let mut actions: Vec<TileAction> = entry
        .actions
        .iter()
        // `validate_codes` also drops actions that would hand the URL to a shell.
        .filter(|a| takes_url(&a.exec) && validate_codes(&a.exec).is_ok())
        .map(|a| TileAction::Desktop {
            id: a.id.clone(),
            label: a.name.clone(),
        })
        .collect();
    if let (Some(a), Some(l)) = (adapter, launch.as_ref())
        && supports(a, l, exec)
        && insert_args(exec, &[a.private.to_owned()], &[]).is_ok()
    {
        actions.push(TileAction::Private);
    }
    let plain = Tile {
        app_id: entry.id.clone(),
        app_name: entry.name.clone(),
        label: entry.name.clone(),
        icon: entry.icon.clone(),
        profile: None,
        flatpak: matches!(launch, Some(Launcher::Flatpak { .. })),
        actions,
    };
    let (Some(adapter), Some(launch)) = (adapter, launch) else {
        return vec![plain];
    };
    if profiles.len() < 2 {
        return vec![plain];
    }
    // Profile tiles only offer actions that open the same browser against the same profile store, and that leave
    // the profile to the tile: one that picks its own could never take the tile's.
    let actions: Vec<TileAction> = plain
        .actions
        .iter()
        .filter(|a| match a {
            TileAction::Desktop { id, .. } => {
                entry.actions.iter().find(|x| &x.id == id).is_some_and(|x| {
                    same_store(adapter, &launch, exec, &x.exec)
                        && insert_args(&x.exec, &[], adapter.store.selectors()).is_ok()
                })
            }
            TileAction::Private => true,
        })
        .cloned()
        .collect();
    profiles
        .iter()
        .map(|p| Tile {
            label: p.label.clone(),
            profile: Some(p.clone()),
            actions: actions.clone(),
            ..plain.clone()
        })
        .collect()
}

/// The tiles of the link picker for `entries`, in picker order and numbered by position.
#[must_use]
pub fn link_tiles(entries: &[AppEntry], env: &Env) -> Vec<Tile> {
    let discovered: Vec<Vec<Profile>> = entries.iter().map(|e| profiles_for(e, env)).collect();
    shown_icons(link_tiles_from(entries, &discovered), env.view)
}

/// [`link_tiles`] for the profiles already discovered per entry, in `entries` order.
#[must_use]
pub fn link_tiles_from(entries: &[AppEntry], discovered: &[Vec<Profile>]) -> Vec<Tile> {
    let mut tiles: Vec<Tile> = entries
        .iter()
        .zip(discovered)
        .flat_map(|(entry, profiles)| tiles_from(entry, profiles))
        .collect();
    disambiguate(&mut tiles);
    tiles
}

/// A Flatpak tile whose label a native tile shares (plain or profile, e.g. native and Flatpak Firefox
/// both with a "Work" profile) gets " (Flatpak)".
pub fn disambiguate(tiles: &mut [Tile]) {
    let marked: Vec<bool> = tiles
        .iter()
        .map(|t| t.flatpak && tiles.iter().any(|o| !o.flatpak && o.label == t.label))
        .collect();
    for (tile, marked) in tiles.iter_mut().zip(marked) {
        if marked {
            tile.label.push_str(" (Flatpak)");
        }
    }
}

#[cfg(test)]
mod discovery_tests;
#[cfg(test)]
mod tests;
