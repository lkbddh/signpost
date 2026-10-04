//! `signpost --host-report`: where a Flatpak Signpost finds the host's files, to check what its grants let it see.

use std::path::{Path, PathBuf};

use super::{Host, HostEnv};

/// What `signpost --host-report` prints for `host`: its environment, or that it is the defaults, then every folder
/// and file Signpost reads (the profile stores of the installed browsers and each profile they list included), each
/// by its host path, then where the sandbox reads it, or that it is out of reach. Only paths are printed.
#[must_use]
pub fn report(host: &Host) -> String {
    let Host::Flatpak(env) = host else {
        return "host: native, not in a Flatpak; the host's files are read where they are\n"
            .to_owned();
    };
    let joined = |dirs: &[PathBuf]| {
        std::env::join_paths(dirs).map_or_else(
            |_| format!("{dirs:?}"),
            |joined| joined.to_string_lossy().into_owned(),
        )
    };
    let mut lines = vec![
        "host: flatpak".to_owned(),
        env.unread.map_or_else(
            || "environment: the host's".to_owned(),
            |why| format!("environment: defaults; the host's could not be read ({why})"),
        ),
        format!("home: {}", env.home.display()),
        format!("config home: {}", env.config_home.display()),
        format!("config dirs: {}", joined(&env.config_dirs)),
        format!("data home: {}", env.data_home.display()),
        format!("data dirs: {}", joined(&env.data_dirs)),
        format!(
            "current desktop: {}",
            env.current_desktop.as_deref().unwrap_or("(none)")
        ),
        format!("scopes: {}", if env.scopes { "yes" } else { "no" }),
    ];
    let application_dirs = crate::index::application_dirs(
        Some(env.data_home.clone()),
        Some(env.data_dirs.clone()),
        &env.home,
    );
    section(&mut lines, env, "application folders", &application_dirs);
    section(&mut lines, env, "default-app lists", &env.mime_list_paths());
    lines.push(match env.user_list() {
        Some(file) => format!("user list: {}", seen(env, &file.path)),
        None => "user list: out of reach".to_owned(),
    });
    let profiles = crate::profiles::Env {
        home: &env.home,
        config_home: &env.config_home,
        view: env,
    };
    lines.push("browser profiles:".to_owned());
    for entry in crate::index::Registry::load_in(env, &application_dirs, &[]).entries() {
        let Some(store) = crate::profiles::store_paths(entry, &profiles) else {
            continue;
        };
        lines.push(format!("  {}:", entry.id));
        lines.extend(
            store
                .folders
                .iter()
                .map(|folder| format!("    store: {}", seen(env, folder))),
        );
        match store.profiles {
            Ok(dirs) => lines.extend(
                dirs.iter()
                    .map(|dir| format!("    profile: {}", seen(env, dir))),
            ),
            Err(e) => lines.push(format!("    unusable: {e}")),
        }
    }
    lines.push(String::new());
    lines.join("\n")
}

fn section(lines: &mut Vec<String>, env: &HostEnv, title: &str, paths: &[PathBuf]) {
    lines.push(format!("{title}:"));
    lines.extend(paths.iter().map(|path| format!("  {}", seen(env, path))));
}

/// `path` on the host, then where the sandbox reads it and in what state, or that it is out of reach.
fn seen(env: &HostEnv, path: &Path) -> String {
    match env.read_path(path) {
        Some(read) => format!(
            "{} -> {} ({})",
            path.display(),
            read.display(),
            state(&read)
        ),
        None => format!("{} -> out of reach", path.display()),
    }
}

/// Whether the sandbox can read what it finds at `read`. Only a folder or a regular file is opened, so a pipe there
/// cannot hold the report.
fn state(read: &Path) -> &'static str {
    let meta = match std::fs::metadata(read) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return "missing",
        Err(_) => return "unreadable",
    };
    let readable = if meta.is_dir() {
        std::fs::read_dir(read).is_ok()
    } else if meta.is_file() {
        crate::index::open_regular(read).is_ok()
    } else {
        return "not a file or folder";
    };
    if readable { "readable" } else { "unreadable" }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::host::HostEnv;

    const ENV: &[u8] =
        b"HOME=/home/u\0XDG_CONFIG_HOME=/home/u/cfg\0XDG_CONFIG_DIRS=/etc/xdg-custom:/tmp/xdg\0\
XDG_DATA_HOME=/home/u/data\0XDG_DATA_DIRS=/opt/share:/usr/share\0XDG_CURRENT_DESKTOP=COSMIC\0\
XDG_ACTIVATION_TOKEN=secret-token\0BROWSER=https://example.org/private\0";

    /// Where the sandbox shows the host path `path` in a fixture under `root`: under `os` for the OS trees, `rest` for
    /// the others.
    fn shown(root: &Path, path: &str) -> std::path::PathBuf {
        let view = if path.starts_with("/usr") || path.starts_with("/etc") {
            "os"
        } else {
            "rest"
        };
        root.join(view).join(path.trim_start_matches('/'))
    }

    /// The host's files under `root`: lists, browsers, and their profile stores.
    fn host_files(root: &Path) {
        for folder in [
            "/usr/share/applications",
            "/home/u/custom-chrome/Default",
            "/home/u/.var/app/org.mozilla.firefox/.mozilla/firefox/abc.default",
            // Where the sandbox has a /tmp of its own: a decoy, never the host's.
            "/tmp/ff-profile",
        ] {
            std::fs::create_dir_all(shown(root, folder)).unwrap();
        }
        let chrome = "[Desktop Entry]\nType=Application\nName=Chrome\n\
                      Exec=/usr/bin/google-chrome-stable --user-data-dir=/home/u/custom-chrome %U\n";
        let firefox = "[Desktop Entry]\nType=Application\nName=Firefox\nExec=/usr/bin/flatpak run --branch=stable \
                       --arch=x86_64 --command=firefox --file-forwarding org.mozilla.firefox @@u %u @@\n";
        let brave = "[Desktop Entry]\nType=Application\nName=Brave\nExec=brave-browser %U\n";
        let local_state =
            r#"{"profile":{"info_cache":{"Default":{"name":"Me"},"Profile 1":{"name":"Work"}}}}"#;
        let profiles_ini = "[Profile0]\nName=default\nIsRelative=1\nPath=abc.default\n\n\
                            [Profile1]\nName=scratch\nIsRelative=0\nPath=/tmp/ff-profile\n";
        for (file, text) in [
            ("/home/u/cfg/mimeapps.list", ""),
            ("/etc/xdg-custom/cosmic-mimeapps.list", ""),
            // A decoy too.
            ("/tmp/xdg/mimeapps.list", ""),
            ("/usr/share/applications/google-chrome.desktop", chrome),
            (
                "/opt/share/applications/org.mozilla.firefox.desktop",
                firefox,
            ),
            ("/opt/share/applications/brave-browser.desktop", brave),
            ("/home/u/custom-chrome/Local State", local_state),
            (
                "/home/u/.var/app/org.mozilla.firefox/.mozilla/firefox/profiles.ini",
                profiles_ini,
            ),
        ] {
            std::fs::create_dir_all(shown(root, file).parent().unwrap()).unwrap();
            std::fs::write(shown(root, file), text).unwrap();
        }
    }

    #[test]
    fn the_report_names_each_host_path_where_the_sandbox_reads_it_or_that_it_cannot() {
        let root = tempfile::tempdir().unwrap();
        let env = HostEnv::parse(ENV, Path::new("/sandbox/home")).mounted_under(root.path());
        host_files(root.path());
        let at = |path: &str, state: &str| {
            format!("{path} -> {} ({state})", shown(root.path(), path).display())
        };
        let firefox_store = "/home/u/.var/app/org.mozilla.firefox/.mozilla/firefox";

        let report = report(&Host::Flatpak(env));

        let lines: Vec<&str> = report.lines().collect();
        for want in [
            "host: flatpak".to_owned(),
            "environment: the host's".to_owned(),
            "home: /home/u".to_owned(),
            "config home: /home/u/cfg".to_owned(),
            "config dirs: /etc/xdg-custom:/tmp/xdg".to_owned(),
            "data home: /home/u/data".to_owned(),
            "data dirs: /opt/share:/usr/share".to_owned(),
            "current desktop: COSMIC".to_owned(),
            "scopes: no".to_owned(),
            format!("  {}", at("/usr/share/applications", "readable")),
            format!("  {}", at("/home/u/data/applications", "missing")),
            format!("  {}", at("/home/u/cfg/mimeapps.list", "readable")),
            format!("  {}", at("/home/u/cfg/cosmic-mimeapps.list", "missing")),
            format!(
                "  {}",
                at("/etc/xdg-custom/cosmic-mimeapps.list", "readable")
            ),
            "  /tmp/xdg/mimeapps.list -> out of reach".to_owned(),
            format!("user list: {}", at("/home/u/cfg/mimeapps.list", "readable")),
            "  google-chrome:".to_owned(),
            format!("    store: {}", at("/home/u/custom-chrome", "readable")),
            format!(
                "    profile: {}",
                at("/home/u/custom-chrome/Default", "readable")
            ),
            format!(
                "    profile: {}",
                at("/home/u/custom-chrome/Profile 1", "missing")
            ),
            "  org.mozilla.firefox:".to_owned(),
            format!(
                "    store: {}",
                at(
                    "/home/u/.var/app/org.mozilla.firefox/config/mozilla/firefox",
                    "missing"
                )
            ),
            format!("    store: {}", at(firefox_store, "readable")),
            format!(
                "    profile: {}",
                at(&format!("{firefox_store}/abc.default"), "readable")
            ),
            "    profile: /tmp/ff-profile -> out of reach".to_owned(),
            "  brave-browser:".to_owned(),
            format!(
                "    store: {}",
                at("/home/u/cfg/BraveSoftware/Brave-Browser", "missing")
            ),
        ] {
            assert!(lines.contains(&want.as_str()), "{want}\n{report}");
        }
        let brave = lines
            .iter()
            .position(|line| *line == "  brave-browser:")
            .unwrap();
        assert!(
            lines[brave + 2].starts_with("    unusable: "),
            "a store that cannot be read says why\n{report}"
        );
        for section in [
            "application folders:",
            "default-app lists:",
            "browser profiles:",
        ] {
            assert!(lines.contains(&section), "{section}\n{report}");
        }
        assert!(
            !report.contains("secret-token") && !report.contains("example.org"),
            "{report}"
        );
    }

    /// `--host-report` runs without logging, so a guessed environment is said in the report itself.
    #[test]
    fn a_report_on_defaults_says_the_hosts_environment_could_not_be_read() {
        let mut env = HostEnv::parse(b"", Path::new("/sandbox/home"));
        env.unread = Some(crate::host::Unread::TimedOut);
        let report = report(&Host::Flatpak(env));
        assert!(
            report
                .lines()
                .any(|line| line
                    == "environment: defaults; the host's could not be read (timed out)"),
            "{report}"
        );
    }

    #[test]
    fn natively_the_report_says_there_is_no_sandbox() {
        assert!(report(&Host::Native).starts_with("host: native"));
    }
}
