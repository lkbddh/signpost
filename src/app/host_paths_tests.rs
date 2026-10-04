//! A Flatpak Signpost finds the host's application folders, lists of default apps and config through the host's
//! environment, with custom XDG locations, and watches them where the sandbox shows them.

use std::path::{Path, PathBuf};

use super::paths::{Paths, mime_list_paths, private_colors, user_list_path, watched};
use crate::host::{Host, HostEnv};

const CUSTOM: &[u8] =
    b"HOME=/home/u\0XDG_CONFIG_HOME=/home/u/cfg\0XDG_CONFIG_DIRS=/etc/xdg-custom:/opt/xdg\0\
XDG_DATA_HOME=/home/u/data\0XDG_DATA_DIRS=/opt/share:/usr/share\0XDG_CURRENT_DESKTOP=COSMIC\0";

/// A Flatpak host with custom XDG locations, its files shown under `root` (`os` for the OS trees, `rest` for the
/// others), and the host files `files` there.
fn flatpak(root: &Path, files: &[&str]) -> Host {
    let env = HostEnv::parse(CUSTOM, Path::new("/sandbox/home")).mounted_under(root);
    for file in files {
        let view = if file.starts_with("/usr") || file.starts_with("/etc") {
            "os"
        } else {
            "rest"
        };
        let path = root.join(view).join(file.trim_start_matches('/'));
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("the folders");
        std::fs::write(path, "").expect("the file");
    }
    Host::Flatpak(env)
}

#[test]
fn custom_xdg_locations_give_the_application_folders_and_the_config_home() {
    let root = tempfile::tempdir().expect("a scratch folder");
    let host = flatpak(root.path(), &[]);
    let paths = Paths::for_host(&host);
    assert_eq!(paths.home, Path::new("/home/u"));
    assert_eq!(paths.config_home, Path::new("/home/u/cfg"));
    assert_eq!(
        paths.data_dirs,
        [
            "/home/u/data/applications",
            "/home/u/.local/share/flatpak/exports/share/applications",
            "/opt/share/applications",
            "/usr/share/applications",
            "/var/lib/flatpak/exports/share/applications",
        ]
        .map(PathBuf::from),
        "host paths, with the Flatpak exports, as index::data_dirs orders them"
    );
}

#[test]
fn custom_xdg_locations_give_the_lists_read_where_the_sandbox_shows_them() {
    let root = tempfile::tempdir().expect("a scratch folder");
    let host = flatpak(
        root.path(),
        &[
            "/home/u/cfg/mimeapps.list",
            "/etc/xdg-custom/cosmic-mimeapps.list",
            "/opt/xdg/mimeapps.list",
            "/usr/share/applications/mimeapps.list",
        ],
    );
    let (os, rest) = (root.path().join("os"), root.path().join("rest"));
    assert_eq!(
        mime_list_paths(&host),
        [
            rest.join("home/u/cfg/mimeapps.list"),
            os.join("etc/xdg-custom/cosmic-mimeapps.list"),
            rest.join("opt/xdg/mimeapps.list"),
            os.join("usr/share/applications/mimeapps.list"),
        ]
    );
    let paths = Paths::for_host(&host);
    assert_eq!(
        user_list_path(&host, &paths),
        Some(rest.join("home/u/cfg/mimeapps.list"))
    );
}

#[test]
fn a_flatpak_watches_the_hosts_folders_where_the_sandbox_shows_them() {
    let root = tempfile::tempdir().expect("a scratch folder");
    let host = flatpak(
        root.path(),
        &["/var/lib/flatpak/exports/share/applications/a.desktop"],
    );
    let paths = Paths::for_host(&host);
    let (os, rest) = (root.path().join("os"), root.path().join("rest"));
    let wanted = watched(&host, &paths);
    assert!(
        wanted.contains(&(rest.join("home/u/data/applications"), true)),
        "{wanted:?}"
    );
    assert!(wanted.contains(&(os.join("usr/share/applications"), true)));
    assert!(wanted.contains(&(
        rest.join("var/lib/flatpak/exports/share/applications"),
        true
    )));
    assert!(wanted.contains(&(rest.join("home/u/cfg"), false)));
    assert!(wanted.contains(&(os.join("etc/xdg-custom"), false)));
    assert!(wanted.contains(&(rest.join("opt/xdg"), false)));
    assert!(
        wanted.iter().all(|(dir, _)| dir.starts_with(root.path())),
        "never a sandbox path: {wanted:?}"
    );
}

#[test]
fn a_flatpak_keeps_its_colors_and_its_record_in_its_own_folders() {
    let root = tempfile::tempdir().expect("a scratch folder");
    let host = flatpak(root.path(), &[]);
    let own_data = Path::new("/home/u/.var/app/com.lkbddh.signpost/data");
    assert_eq!(
        private_colors(&host, own_data),
        Some(own_data.join("signpost/colors")),
        "never the host's config, which the bundle may write"
    );
    assert_eq!(
        private_colors(&Host::Native, own_data),
        None,
        "natively, COSMIC's config"
    );
    assert_eq!(
        Paths::for_host(&host).state_home,
        Paths::from_env().state_home,
        "the restore record stays in the process's own state folder"
    );
}

/// The XDG spec has a relative path in these variables ignored: the restore record and the user's list never land
/// under the folder Signpost started in.
#[test]
fn relative_xdg_folders_fall_back_to_the_home() {
    let var = |name: &str| {
        matches!(name, "XDG_CONFIG_HOME" | "XDG_STATE_HOME" | "XDG_DATA_HOME")
            .then(|| "rel".to_owned())
    };
    let paths = Paths::from_vars(&var, PathBuf::from("/home/u"));
    assert_eq!(paths.config_home, Path::new("/home/u/.config"));
    assert_eq!(paths.state_home, Path::new("/home/u/.local/state"));
    assert_eq!(paths.own_data, Path::new("/home/u/.local/share"));
    let var = |name: &str| (name == "XDG_STATE_HOME").then(|| "/s".to_owned());
    assert_eq!(
        Paths::from_vars(&var, PathBuf::from("/home/u")).state_home,
        Path::new("/s")
    );
}
