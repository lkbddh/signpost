//! Where Signpost finds what it reads and writes: the user's folders natively or the host's in a Flatpak, the lists of
//! default apps, the folders it watches, and the store of the saved profile colours.

use std::path::{Path, PathBuf};

use crate::colors;
use crate::host::Host;
use crate::index::{application_dirs, data_dirs};

/// Environment-derived paths, resolved once at startup.
pub(super) struct Paths {
    pub(super) home: PathBuf,
    pub(super) config_home: PathBuf,
    pub(super) state_home: PathBuf,
    /// This process's own data folder, which in a Flatpak is the sandbox's.
    pub(super) own_data: PathBuf,
    pub(super) data_dirs: Vec<PathBuf>,
}

/// The user's home folder: `$HOME`, or the user database's while that is unset. A relative one is none.
pub(super) fn home_dir() -> Option<PathBuf> {
    std::env::home_dir().filter(|home| home.is_absolute())
}

impl Paths {
    /// [`Paths::from_vars`] of this process's environment; [`run`] starts nothing without a home.
    pub(super) fn from_env() -> Self {
        Self::from_vars(
            &|name| std::env::var(name).ok(),
            home_dir().unwrap_or_default(),
        )
    }

    /// The paths `var` names, defaulted under `home`. An XDG folder counts only when absolute, as the spec has it.
    pub(super) fn from_vars(var: &dyn Fn(&str) -> Option<String>, home: PathBuf) -> Self {
        let folder = |name: &str| {
            var(name)
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
        };
        Self {
            config_home: folder("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config")),
            state_home: folder("XDG_STATE_HOME").unwrap_or_else(|| home.join(".local/state")),
            own_data: folder("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local/share")),
            data_dirs: data_dirs(var, &home),
            home,
        }
    }

    /// The paths of `host`: natively the environment's, as [`Paths::from_env`] reads it; in a Flatpak, the host's
    /// home, config home and application folders, and the sandbox's own state.
    pub(super) fn for_host(host: &Host) -> Self {
        let Host::Flatpak(env) = host else {
            return Self::from_env();
        };
        Self {
            home: env.home.clone(),
            config_home: env.config_home.clone(),
            data_dirs: application_dirs(
                Some(env.data_home.clone()),
                Some(env.data_dirs.clone()),
                &env.home,
            ),
            ..Self::from_env()
        }
    }

    fn user_list(&self) -> PathBuf {
        cosmic_mime_apps::local_list_path()
            .unwrap_or_else(|| self.config_home.join("mimeapps.list"))
    }
}

/// Where `host`'s files are read: as given natively, through the sandbox's view of the host in a Flatpak.
pub(super) fn view_of(host: &Host) -> &dyn crate::index::FsView {
    match host {
        Host::Native => &crate::index::Native,
        Host::Flatpak(env) => env,
    }
}

/// Every `mimeapps.list`, highest precedence first, where this process reads them.
pub(super) fn mime_list_paths(host: &Host) -> Vec<PathBuf> {
    match host {
        Host::Native => cosmic_mime_apps::list_paths(),
        Host::Flatpak(env) => env.mime_lists().into_iter().map(|file| file.read).collect(),
    }
}

/// The list setup writes, where this process writes it; `None` when a Flatpak cannot reach the host's.
pub(super) fn user_list_path(host: &Host, paths: &Paths) -> Option<PathBuf> {
    match host {
        Host::Native => Some(paths.user_list()),
        Host::Flatpak(env) => env.user_list().map(|file| file.read),
    }
}

/// The folders to watch for changes to the index or the defaults, where this process sees them, each with whether
/// its subfolders are watched too: desktop entries live in nested folders (the index reads them recursively), and a
/// `mimeapps.list` may sit in the config home or any config dir.
pub(super) fn watched(host: &Host, paths: &Paths) -> Vec<(PathBuf, bool)> {
    let Host::Flatpak(env) = host else {
        let config_dirs = std::env::var("XDG_CONFIG_DIRS")
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "/etc/xdg".to_owned());
        return paths
            .data_dirs
            .iter()
            .map(|d| (d.clone(), true))
            .chain(
                std::iter::once(paths.config_home.clone())
                    .chain(
                        config_dirs
                            .split(':')
                            .map(PathBuf::from)
                            .filter(|dir| dir.is_absolute()),
                    )
                    .map(|d| (d, false)),
            )
            .collect();
    };
    let shown = |dirs: Vec<PathBuf>, nested: bool| {
        dirs.into_iter()
            .filter_map(move |dir| env.read_path(&dir).map(|read| (read, nested)))
    };
    shown(paths.data_dirs.clone(), true)
        .chain(shown(
            std::iter::once(env.config_home.clone())
                .chain(env.config_dirs.iter().cloned())
                .collect(),
            false,
        ))
        .collect()
}

/// Where a Flatpak keeps the profile colors: in its own data folder `own_data`, as the config folder it is granted is
/// the host's; `None` natively, where they go in COSMIC's config.
pub(super) fn private_colors(host: &Host, own_data: &Path) -> Option<PathBuf> {
    matches!(host, Host::Flatpak(_)).then(|| own_data.join("signpost/colors"))
}

/// The saved profile color choices; whatever cannot be read leaves the browsers' own colors showing.
pub(super) fn load_colors(
    host: &Host,
    own_data: &Path,
) -> (Option<colors::ColorStore>, colors::Overrides) {
    let opened = match private_colors(host, own_data) {
        Some(dir) => colors::ColorStore::at(dir),
        None => colors::ColorStore::open(),
    };
    let store = match opened {
        Ok(store) => store,
        Err(error) => {
            tracing::warn!(%error, "profile colors cannot be saved; showing the browsers' own");
            return (None, colors::Overrides::default());
        }
    };
    colors::loaded(store)
}
