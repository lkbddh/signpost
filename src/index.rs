use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cosmic::desktop::fde;

use crate::launch::exec::{ExecError, takes_url, tokenize, unescape_value, validate_codes};

pub const SELF_ID: &str = "com.lkbddh.signpost";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppAction {
    pub id: String,
    pub name: String,
    pub exec: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "mirrors the Desktop Entry boolean keys, one field each"
)]
pub struct AppEntry {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub exec: Option<Vec<String>>,
    pub exec_error: Option<ExecError>,
    pub try_exec: Option<String>,
    pub work_dir: Option<PathBuf>,
    pub terminal: bool,
    pub dbus_activatable: bool,
    pub no_display: bool,
    pub hidden: bool,
    pub mime_types: Vec<String>,
    pub actions: Vec<AppAction>,
    pub file: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct Registry {
    entries: Vec<AppEntry>,
}

fn split_list(v: Option<Vec<&str>>) -> Vec<String> {
    v.unwrap_or_default()
        .into_iter()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `path` opened for reading if it is a regular file, and without blocking, so a pipe or device there, or one put
/// there in between, can never hold the caller.
///
/// # Errors
/// The error of looking `path` up or opening it ([`std::io::ErrorKind::NotFound`] when nothing is there), or
/// [`std::io::ErrorKind::InvalidInput`] for anything but a regular file.
pub fn open_regular(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt as _;
    let not_a_file = || std::io::Error::new(std::io::ErrorKind::InvalidInput, "not a regular file");
    if !std::fs::metadata(path)?.is_file() {
        return Err(not_a_file());
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(rustix::fs::OFlags::NONBLOCK.bits().cast_signed())
        .open(path)?;
    if file.metadata()?.is_file() {
        Ok(file)
    } else {
        Err(not_a_file())
    }
}

/// The text of `path`, read only if it is a regular file, as [`open_regular`] opens it.
///
/// # Errors
/// The errors of [`open_regular`], or of reading the file.
pub fn read_regular(path: &Path) -> std::io::Result<String> {
    let mut text = String::new();
    std::io::Read::read_to_string(&mut open_regular(path)?, &mut text)?;
    Ok(text)
}

/// The lists of default apps at `paths`, merged as `cosmic_mime_apps::List::load_from_paths` merges them, the first
/// list's entry for a type winning; a path that is not a regular file is skipped rather than read.
#[must_use]
pub fn load_lists(paths: &[impl AsRef<Path>]) -> cosmic_mime_apps::List {
    let mut list = cosmic_mime_apps::List::default();
    for path in paths {
        if let Ok(text) = read_regular(path.as_ref()) {
            list.load_from(&text);
        }
    }
    list
}

/// How the index sees the files it reads: as this process sees them, or the host's through a Flatpak sandbox.
pub trait FsView {
    /// Where to read the file or folder at `path`, or `None` where it cannot be read.
    fn read_path(&self, path: &Path) -> Option<PathBuf>;
    /// What tells the folder `dir` apart from every other however it is reached: its path with every link followed.
    fn identity(&self, dir: &Path) -> Option<PathBuf>;
}

/// The files as this process sees them.
pub struct Native;

impl FsView for Native {
    fn read_path(&self, path: &Path) -> Option<PathBuf> {
        Some(path.to_owned())
    }

    fn identity(&self, dir: &Path) -> Option<PathBuf> {
        std::fs::canonicalize(dir).ok()
    }
}

/// The entry `id` the desktop file at `file` holds, as `de` read it.
fn entry_from(id: String, file: PathBuf, de: &fde::DesktopEntry, locales: &[String]) -> AppEntry {
    let (exec, exec_error) = match de
        .exec()
        .map(|e| tokenize(&unescape_value(e)).and_then(|t| validate_codes(&t).map(|()| t)))
    {
        Some(Ok(argv)) => (Some(argv), None),
        Some(Err(e)) => (None, Some(e)),
        None => (None, None),
    };
    let actions = split_list(de.actions())
        .into_iter()
        .filter_map(|id| {
            let exec = tokenize(&unescape_value(de.action_exec(&id)?)).ok()?;
            let name = de
                .action_name(&id, locales)
                .map_or_else(|| id.clone(), Cow::into_owned);
            Some(AppAction { id, name, exec })
        })
        .collect();
    AppEntry {
        name: de.name(locales).map_or_else(|| id.clone(), Cow::into_owned),
        id,
        icon: de.icon().map(str::to_owned),
        exec,
        exec_error,
        try_exec: de.try_exec().map(unescape_value),
        work_dir: de.path().map(PathBuf::from),
        terminal: de.terminal(),
        dbus_activatable: de.dbus_activatable(),
        no_display: de.no_display(),
        hidden: de.hidden(),
        mime_types: split_list(de.mime_type()),
        actions,
        file,
    }
}

/// Directory depth below a root that is still walked; a backstop behind the visited-directory check.
const MAX_DEPTH: usize = 16;
/// How many folders one application folder's walk reads at most, its subfolders and linked folders included.
const MAX_FOLDERS: usize = 4096;

/// Collects `(desktop file id, path, read path)` under `dir` in sorted order, through `view`. Paths are walked
/// as given, following symlinks. A directory is skipped only when its identity is already on the active ancestor
/// chain (`ancestors`), which cuts true symlink cycles while siblings and aliases keep their own logical names.
/// Only regular files count, so a FIFO or device never gets opened. The id is the path relative to the root, `/`
/// replaced by `-`, without the `.desktop` suffix (desktop-file-id rule); `prefix` is the already-joined
/// directory part.
fn collect_desktop_files(
    view: &dyn FsView,
    dir: &Path,
    prefix: &str,
    depth: usize,
    ancestors: &mut Vec<PathBuf>,
    out: &mut Vec<(String, PathBuf, PathBuf)>,
    budget: &mut usize,
) {
    let Some(canonical) = view.identity(dir) else {
        return;
    };
    if ancestors.contains(&canonical) {
        return;
    }
    // Folders linked several ways at every level multiply the paths to walk; past the budget the rest are left.
    let Some(left) = budget.checked_sub(1) else {
        return;
    };
    *budget = left;
    let Some(Ok(read)) = view.read_path(dir).map(std::fs::read_dir) else {
        return;
    };
    ancestors.push(canonical);
    let mut names: Vec<_> = read.filter_map(Result::ok).map(|e| e.file_name()).collect();
    names.sort();
    for name in names {
        let Some(name) = name.to_str() else { continue };
        let path = dir.join(name);
        let Some(read_path) = view.read_path(&path) else {
            continue;
        };
        let Ok(meta) = std::fs::metadata(&read_path) else {
            continue;
        };
        if meta.is_dir() {
            if depth < MAX_DEPTH {
                let prefix = format!("{prefix}{name}-");
                collect_desktop_files(view, &path, &prefix, depth + 1, ancestors, out, budget);
            }
        } else if meta.is_file()
            && let Some(stem) = name.strip_suffix(".desktop")
        {
            out.push((format!("{prefix}{stem}"), path, read_path));
        }
    }
    ancestors.pop();
}

impl Registry {
    /// `dirs` in precedence order (highest first); the first entry for an id wins.
    #[must_use]
    pub fn load(dirs: &[PathBuf], locales: &[String]) -> Self {
        Self::load_in(&Native, dirs, locales)
    }

    /// [`Registry::load`] through `view`: each entry's `file` is its path as given, read where `view` says.
    #[must_use]
    pub fn load_in(view: &dyn FsView, dirs: &[PathBuf], locales: &[String]) -> Self {
        let mut entries: Vec<AppEntry> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for dir in dirs {
            let mut files = Vec::new();
            let mut budget = MAX_FOLDERS;
            collect_desktop_files(view, dir, "", 0, &mut Vec::new(), &mut files, &mut budget);
            if budget == 0 {
                tracing::warn!(dir = %dir.display(), "too many folders to walk; the rest of this one is left out");
            }
            for (id, path, read_path) in files {
                if !seen.insert(id.clone()) {
                    continue;
                }
                let Ok(de) = read_regular(&read_path).map_err(|_| ()).and_then(|text| {
                    fde::DesktopEntry::from_str(&read_path, &text, Some(locales)).map_err(|_| ())
                }) else {
                    continue;
                };
                entries.push(entry_from(id, path, &de, locales));
            }
        }
        Self { entries }
    }

    /// Accepts ids with or without the `.desktop` suffix.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&AppEntry> {
        let id = id.strip_suffix(".desktop").unwrap_or(id);
        self.entries.iter().find(|e| e.id == id)
    }

    #[must_use]
    pub fn entries(&self) -> &[AppEntry] {
        &self.entries
    }
}

/// Application dirs in precedence order: `$XDG_DATA_HOME` (default `~/.local/share`), the user Flatpak
/// exports, every `$XDG_DATA_DIRS` entry (default `/usr/local/share:/usr/share`), the system Flatpak
/// exports. Each gets `/applications` appended; duplicates are removed keeping the first.
pub fn data_dirs(env: &dyn Fn(&str) -> Option<String>, home: &Path) -> Vec<PathBuf> {
    // The XDG spec has a relative path in these variables ignored.
    let data_home = env("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute());
    let data_dirs = env("XDG_DATA_DIRS")
        .map(|dirs| {
            dirs.split(':')
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .collect::<Vec<_>>()
        })
        .filter(|dirs| !dirs.is_empty());
    application_dirs(data_home, data_dirs, home)
}

/// [`data_dirs`] from the data home and data dirs themselves, each defaulted when `None`.
#[must_use]
pub fn application_dirs(
    data_home: Option<PathBuf>,
    data_dirs: Option<Vec<PathBuf>>,
    home: &Path,
) -> Vec<PathBuf> {
    let data_home = data_home.unwrap_or_else(|| home.join(".local/share"));
    let data_dirs = data_dirs.unwrap_or_else(|| {
        vec![
            PathBuf::from("/usr/local/share"),
            PathBuf::from("/usr/share"),
        ]
    });
    let mut bases = vec![data_home, home.join(".local/share/flatpak/exports/share")];
    bases.extend(data_dirs);
    bases.push(PathBuf::from("/var/lib/flatpak/exports/share"));
    let mut out: Vec<PathBuf> = Vec::new();
    for dir in bases.into_iter().map(|b| b.join("applications")) {
        if !out.contains(&dir) {
            out.push(dir);
        }
    }
    out
}

/// Parsed `mimeapps.list` files, merged with the highest-precedence file winning per MIME type.
pub struct MimeLists {
    /// Each list on its own, highest precedence first.
    lists: Vec<cosmic_mime_apps::List>,
}

impl MimeLists {
    /// The lists at `paths`, highest precedence first.
    #[must_use]
    pub fn load(paths: &[PathBuf]) -> Self {
        Self {
            lists: paths
                .iter()
                .map(|path| load_lists(std::slice::from_ref(path)))
                .collect(),
        }
    }

    fn ids(map: &BTreeMap<mime::Mime, Vec<Box<str>>>, mime: &mime::Mime) -> Vec<String> {
        map.get(mime)
            .map(|v| {
                v.iter()
                    .map(|s| s.strip_suffix(".desktop").unwrap_or(s).to_owned())
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Link mode: not Signpost, not `Hidden`/`NoDisplay`, and either `Exec` with `%u`/`%U` or `DBusActivatable`, whose
/// `Open` method takes the link whatever its compatibility `Exec` takes, with an `Exec` that parsed or none.
#[must_use]
pub fn is_link_eligible(entry: &AppEntry) -> bool {
    entry.id != SELF_ID
        && !entry.hidden
        && !entry.no_display
        && match &entry.exec {
            Some(argv) => takes_url(argv) || entry.dbus_activatable,
            None => entry.dbus_activatable && entry.exec_error.is_none(),
        }
}

impl Registry {
    /// Each list in precedence order, its default then its added associations, then the entries declaring the
    /// type (by name, then id); duplicates removed, only link-eligible entries returned. A list's removed
    /// associations hide an app from that list and every lower one and from the declarations, as the MIME
    /// applications spec has them, never from a higher list.
    #[must_use]
    pub fn link_candidates(&self, mime: &str, lists: &MimeLists) -> Vec<&AppEntry> {
        let Ok(m) = mime.parse::<mime::Mime>() else {
            return Vec::new();
        };
        let mut removed: Vec<String> = Vec::new();
        let mut listed: Vec<String> = Vec::new();
        for list in &lists.lists {
            removed.extend(MimeLists::ids(&list.removed_associations, &m));
            listed.extend(
                MimeLists::ids(&list.default_apps, &m)
                    .into_iter()
                    .chain(MimeLists::ids(&list.added_associations, &m))
                    .filter(|id| !removed.contains(id)),
            );
        }
        let mut declared: Vec<&AppEntry> = self
            .entries
            .iter()
            .filter(|e| e.mime_types.iter().any(|t| t == mime) && !removed.contains(&e.id))
            .collect();
        declared.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
        let ordered = listed.iter().filter_map(|id| self.get(id)).chain(declared);
        let mut out: Vec<&AppEntry> = Vec::new();
        for entry in ordered {
            if out.iter().any(|e| e.id == entry.id) || !is_link_eligible(entry) {
                continue;
            }
            out.push(entry);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn a_pipe_is_never_read_as_a_file_and_a_missing_file_says_so() {
        use std::io::ErrorKind;
        let tmp = tempfile::tempdir().unwrap();
        let (pipe, missing) = (tmp.path().join("pipe"), tmp.path().join("missing"));
        crate::test_support::pipe_at(&pipe);
        let kinds = crate::test_support::finishes(move || {
            [read_regular(&pipe), read_regular(&missing)].map(|read| read.unwrap_err().kind())
        });
        assert_eq!(kinds, Some([ErrorKind::InvalidInput, ErrorKind::NotFound]));
    }

    #[test]
    fn lists_load_around_a_pipe_among_them() {
        let tmp = tempfile::tempdir().unwrap();
        let (first, pipe, last) = (
            tmp.path().join("a.list"),
            tmp.path().join("pipe.list"),
            tmp.path().join("b.list"),
        );
        std::fs::write(
            &first,
            "[Default Applications]\nx-scheme-handler/https=a.desktop\n",
        )
        .unwrap();
        std::fs::write(
            &last,
            "[Default Applications]\nx-scheme-handler/http=b.desktop\n",
        )
        .unwrap();
        crate::test_support::pipe_at(&pipe);
        let loaded = crate::test_support::finishes(move || {
            let merged = load_lists(&[first, pipe, last]);
            let named = |scheme: &str| {
                merged
                    .default_app_for(&format!("x-scheme-handler/{scheme}").parse().unwrap())
                    .map(|apps| apps.iter().map(ToString::to_string).collect::<Vec<_>>())
            };
            (named("https"), named("http"))
        });
        assert_eq!(
            loaded,
            Some((
                Some(vec!["a.desktop".to_owned()]),
                Some(vec!["b.desktop".to_owned()])
            ))
        );
    }
    #[test]
    fn higher_precedence_dir_wins() {
        let r = registry();
        assert_eq!(r.get("firefox").unwrap().name, "Firefox (user)");
        assert_eq!(r.get("firefox.desktop").unwrap().name, "Firefox (user)");
        assert_eq!(r.entries().iter().filter(|e| e.id == "firefox").count(), 1);
    }

    #[test]
    fn parses_exec_actions_and_flags() {
        let r = registry();
        let ff = r.get("firefox").unwrap();
        assert_eq!(
            ff.exec.as_deref(),
            Some(&["firefox".to_owned(), "%u".to_owned()][..])
        );
        assert_eq!(ff.icon.as_deref(), Some("firefox"));
        assert!(ff.mime_types.contains(&"x-scheme-handler/https".to_owned()));
        assert_eq!(
            ff.actions,
            vec![
                AppAction {
                    id: "new-window".into(),
                    name: "New Window".into(),
                    exec: vec!["firefox".into(), "--new-window".into(), "%u".into()]
                },
                AppAction {
                    id: "private-window".into(),
                    name: "New Private Window".into(),
                    exec: vec!["firefox".into(), "--private-window".into(), "%u".into()]
                },
            ]
        );
        assert!(r.get("com.google.Chrome").unwrap().no_display);
        assert!(r.get("hidden").unwrap().hidden);
        let dbus = r.get("org.example.DbusOnly").unwrap();
        assert!(dbus.dbus_activatable && dbus.exec.is_none());
        let bad = r.get("badexec").unwrap();
        assert!(bad.exec.is_none() && bad.exec_error == Some(ExecError::Unterminated));
        let badcode = r.get("badcode").unwrap();
        assert!(
            badcode.exec.is_none() && badcode.exec_error == Some(ExecError::UnknownFieldCode('x'))
        );
        let term = r.get("term").unwrap();
        assert!(term.terminal);
        assert_eq!(term.work_dir.as_deref(), Some(Path::new("/tmp")));
        assert_eq!(term.try_exec.as_deref(), Some("lynx"));
        assert!(r.get("missing").is_none());
    }

    #[test]
    fn exec_values_are_unescaped_before_tokenizing() {
        let r = registry();
        let e = r.get("escaped").unwrap();
        assert_eq!(
            e.exec.as_deref(),
            Some(
                &[
                    "printer".to_owned(),
                    r"a\b".to_owned(),
                    "$HOME".to_owned(),
                    "%u".to_owned()
                ][..]
            )
        );
        assert_eq!(e.actions[0].exec, ["printer", r"c\d", "%u"]);
    }

    fn write_entry(path: &Path, name: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            path,
            format!("[Desktop Entry]\nType=Application\nName={name}\nExec=x %u\n"),
        )
        .unwrap();
    }

    #[test]
    fn symlinked_root_keeps_logical_nested_ids() {
        let tmp = tempfile::tempdir().unwrap();
        write_entry(&tmp.path().join("export/foo/bar.desktop"), "Bar");
        let root = tmp.path().join("applications");
        std::os::unix::fs::symlink(tmp.path().join("export"), &root).unwrap();
        let r = Registry::load(&[root], &["en".into()]);
        assert_eq!(r.get("foo-bar").unwrap().name, "Bar");
        assert!(r.get("bar").is_none());
    }

    #[test]
    fn nested_entry_overrides_flat_entry_of_the_same_id() {
        let tmp = tempfile::tempdir().unwrap();
        let (high, low) = (tmp.path().join("high"), tmp.path().join("low"));
        write_entry(&high.join("foo/bar.desktop"), "High");
        write_entry(&low.join("foo-bar.desktop"), "Low");
        let r = Registry::load(&[high, low], &["en".into()]);
        assert_eq!(r.get("foo-bar").unwrap().name, "High");
        assert_eq!(r.entries().len(), 1);
    }

    /// Runs `Registry::load` on a thread so a hang or blow-up fails the test instead of stalling it.
    fn load_within_2s(root: &Path) -> Registry {
        let root = root.to_path_buf();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(Registry::load(&[root], &["en".into()]));
        });
        rx.recv_timeout(std::time::Duration::from_secs(2))
            .expect("Registry::load did not finish within 2 s")
    }

    #[test]
    fn aliased_dirs_keep_their_logical_ids_and_precedence() {
        let tmp = tempfile::tempdir().unwrap();
        let (high, low) = (tmp.path().join("high"), tmp.path().join("low"));
        std::fs::create_dir_all(high.join("real")).unwrap();
        std::fs::write(
            high.join("real/app.desktop"),
            "[Desktop Entry]\nType=Application\nName=High\nExec=x %u\nMimeType=x-scheme-handler/https;\nHidden=true\n",
        )
        .unwrap();
        std::os::unix::fs::symlink(high.join("real"), high.join("alias")).unwrap();
        write_entry(&low.join("real-app.desktop"), "Low");
        let r = Registry::load(&[high, low], &["en".into()]);
        let shadow = r.get("real-app").unwrap();
        assert!(shadow.hidden && shadow.name == "High");
        assert_eq!(r.get("alias-app").unwrap().name, "High");
    }

    #[test]
    fn symlink_cycles_are_walked_once() {
        let tmp = tempfile::tempdir().unwrap();
        write_entry(&tmp.path().join("a.desktop"), "A");
        for link in ["loop1", "loop2"] {
            std::os::unix::fs::symlink(tmp.path(), tmp.path().join(link)).unwrap();
        }
        let r = load_within_2s(tmp.path());
        assert_eq!(r.entries().len(), 1);
        assert_eq!(r.get("a").unwrap().name, "A");
    }

    #[test]
    fn non_regular_desktop_files_are_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        write_entry(&tmp.path().join("ok.desktop"), "Ok");
        let fifo = tmp.path().join("blocked.desktop");
        // No fork: a child holding another test's fresh executable open would make its exec fail ETXTBSY.
        rustix::fs::mkfifoat(
            rustix::fs::CWD,
            &fifo,
            rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
        )
        .unwrap();
        let r = load_within_2s(tmp.path());
        assert!(r.get("blocked").is_none());
        assert_eq!(r.get("ok").unwrap().name, "Ok");
    }

    #[test]
    fn an_exec_handing_a_url_code_to_a_shell_is_not_link_eligible() {
        let tmp = tempfile::tempdir().unwrap();
        for (name, exec) in [
            ("embedded", r#"sh -c "firefox %u""#),
            ("positional", r#"sh -c 'firefox "$1"' sh %u"#),
            ("wrapped", "env MOZ_X=1 firefox %u"),
        ] {
            std::fs::write(
                tmp.path().join(format!("{name}.desktop")),
                format!("[Desktop Entry]\nType=Application\nName={name}\nExec={exec}\nMimeType=x-scheme-handler/https;\n"),
            )
            .unwrap();
        }
        let r = Registry::load(&[tmp.path().to_path_buf()], &["en".into()]);
        for refused in ["embedded", "positional"] {
            let e = r.get(refused).unwrap();
            assert_eq!(
                e.exec_error,
                Some(ExecError::ShellReceivesFieldCode),
                "{refused}"
            );
            assert!(!is_link_eligible(e), "{refused}");
        }
        assert!(is_link_eligible(r.get("wrapped").unwrap()));
    }

    /// A Flatpak's view of a host whose files the sandbox shows under `root`.
    fn flatpak_view(root: &Path) -> crate::host::HostEnv {
        crate::host::HostEnv::parse(b"HOME=/home/u\0", Path::new("/sandbox/home"))
            .mounted_under(root)
    }

    /// `Registry::load_in` through `view` on a thread, so a hang fails the test instead of stalling it.
    fn load_through(view: crate::host::HostEnv, dirs: &[&str]) -> Registry {
        let dirs: Vec<PathBuf> = dirs.iter().map(PathBuf::from).collect();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(Registry::load_in(&view, &dirs, &["en".into()]));
        });
        rx.recv_timeout(std::time::Duration::from_secs(2))
            .expect("Registry::load_in did not finish within 2 s")
    }

    #[test]
    fn the_flatpak_view_keeps_the_walkers_safeguards_and_logical_ids() {
        let root = tempfile::tempdir().unwrap();
        let rest = root.path().join("rest/home/u");
        std::fs::create_dir_all(rest.join("high/real")).unwrap();
        std::fs::write(
            rest.join("high/real/app.desktop"),
            "[Desktop Entry]\nType=Application\nName=High\nExec=x %u\nMimeType=x-scheme-handler/https;\nHidden=true\n",
        )
        .unwrap();
        std::os::unix::fs::symlink("real", rest.join("high/alias")).unwrap();
        write_entry(&rest.join("low/real-app.desktop"), "Low");
        write_entry(&rest.join("loops/a.desktop"), "A");
        for link in ["loop1", "loop2"] {
            std::os::unix::fs::symlink(".", rest.join("loops").join(link)).unwrap();
        }
        write_entry(&rest.join("fifo/ok.desktop"), "Ok");
        rustix::fs::mkfifoat(
            rustix::fs::CWD,
            rest.join("fifo/blocked.desktop"),
            rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
        )
        .unwrap();
        let r = load_through(
            flatpak_view(root.path()),
            &[
                "/home/u/high",
                "/home/u/low",
                "/home/u/loops",
                "/home/u/fifo",
            ],
        );
        let shadow = r.get("real-app").unwrap();
        assert!(shadow.hidden && shadow.name == "High", "precedence");
        assert_eq!(
            r.get("alias-app").unwrap().name,
            "High",
            "an alias keeps its own id"
        );
        assert_eq!(r.get("a").unwrap().name, "A");
        assert!(
            r.get("loop1-a").is_none() && r.get("loop2-a").is_none(),
            "a cycle is walked once"
        );
        assert!(r.get("blocked").is_none(), "a FIFO is never opened");
        assert_eq!(r.get("ok").unwrap().name, "Ok");
        assert_eq!(
            r.get("real-app").unwrap().file,
            Path::new("/home/u/high/real/app.desktop"),
            "an entry's file is the host's path"
        );
    }

    #[test]
    fn the_flatpak_view_reads_system_exports_and_never_the_sandboxs_own_apps() {
        let root = tempfile::tempdir().unwrap();
        let rest = root.path().join("rest");
        let exports = rest.join("var/lib/flatpak/exports/share/applications");
        write_entry(
            &rest.join(
                "var/lib/flatpak/app/org.example.App/current/active/export/share/applications/org.example.App.desktop",
            ),
            "Exported",
        );
        std::fs::create_dir_all(&exports).unwrap();
        std::os::unix::fs::symlink(
            "../../../app/org.example.App/current/active/export/share/applications/org.example.App.desktop",
            exports.join("org.example.App.desktop"),
        )
        .unwrap();
        write_entry(
            &root.path().join("os/usr/share/applications/host.desktop"),
            "Host",
        );
        write_entry(
            &rest.join("usr/share/applications/runtime.desktop"),
            "Runtime decoy",
        );
        let r = load_through(
            flatpak_view(root.path()),
            &[
                "/var/lib/flatpak/exports/share/applications",
                "/usr/share/applications",
            ],
        );
        let exported = r.get("org.example.App").unwrap();
        assert_eq!(exported.name, "Exported");
        assert_eq!(
            exported.file,
            Path::new("/var/lib/flatpak/exports/share/applications/org.example.App.desktop")
        );
        assert_eq!(r.get("host").unwrap().name, "Host");
        assert!(
            r.get("runtime").is_none(),
            "the sandbox's own /usr is never read"
        );
    }

    #[test]
    fn data_dirs_defaults_and_order() {
        let home = Path::new("/home/u");
        let empty = |_: &str| None;
        assert_eq!(
            data_dirs(&empty, home),
            [
                "/home/u/.local/share/applications",
                "/home/u/.local/share/flatpak/exports/share/applications",
                "/usr/local/share/applications",
                "/usr/share/applications",
                "/var/lib/flatpak/exports/share/applications",
            ]
            .map(PathBuf::from)
        );
        let env = |k: &str| match k {
            "XDG_DATA_HOME" => Some("/d/home".to_owned()),
            "XDG_DATA_DIRS" => Some("/x:/var/lib/flatpak/exports/share:/y".to_owned()),
            _ => None,
        };
        assert_eq!(
            data_dirs(&env, home),
            [
                "/d/home/applications",
                "/home/u/.local/share/flatpak/exports/share/applications",
                "/x/applications",
                "/var/lib/flatpak/exports/share/applications",
                "/y/applications",
            ]
            .map(PathBuf::from)
        );
    }

    /// The XDG spec has a relative path in these variables ignored, so nothing is read from where Signpost started.
    #[test]
    fn relative_data_dirs_are_ignored() {
        let home = Path::new("/home/u");
        let env = |k: &str| match k {
            "XDG_DATA_HOME" => Some("data".to_owned()),
            "XDG_DATA_DIRS" => Some("share:/x".to_owned()),
            _ => None,
        };
        assert_eq!(
            data_dirs(&env, home),
            [
                "/home/u/.local/share/applications",
                "/home/u/.local/share/flatpak/exports/share/applications",
                "/x/applications",
                "/var/lib/flatpak/exports/share/applications",
            ]
            .map(PathBuf::from)
        );
    }

    #[test]
    fn candidate_order_and_filters() {
        let r = registry();
        let lists = MimeLists::load(&[
            fixtures().join("mimeapps/high.list"),
            fixtures().join("mimeapps/low.list"),
        ]);
        let ids: Vec<&str> = r
            .link_candidates("x-scheme-handler/https", &lists)
            .iter()
            .map(|e| e.id.as_str())
            .collect();
        // Each list in turn, its default then its additions, then the apps that only declare the type: the low
        // list's default is an association it makes, ahead of a declaration.
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
    }

    #[test]
    fn eligibility() {
        let r = registry();
        for excluded in [
            "com.google.Chrome",
            "hidden",
            "noexec",
            "badexec",
            "badcode",
            "com.lkbddh.signpost",
            "filesonly",
        ] {
            assert!(
                !is_link_eligible(r.get(excluded).unwrap()),
                "{excluded} must be excluded"
            );
        }
        for included in [
            "firefox",
            "google-chrome",
            "org.example.DbusOnly",
            "env-browser",
            "term",
        ] {
            assert!(
                is_link_eligible(r.get(included).unwrap()),
                "{included} must be eligible"
            );
        }
    }

    /// A D-Bus activatable app takes links through its `Open` method, whatever its compatibility `Exec` takes.
    #[test]
    fn a_dbus_app_whose_exec_takes_no_link_is_eligible() {
        let mut app = registry().get("filesonly").unwrap().clone();
        assert!(!is_link_eligible(&app));
        app.dbus_activatable = true;
        assert!(is_link_eligible(&app));
    }

    #[test]
    fn unknown_mime_and_missing_lists() {
        let r = registry();
        let none = MimeLists::load(&[fixtures().join("mimeapps/does-not-exist.list")]);
        assert_eq!(
            r.link_candidates("x-scheme-handler/gopher", &none),
            Vec::<&AppEntry>::new()
        );
        let ids: Vec<&str> = r
            .link_candidates("x-scheme-handler/https", &none)
            .iter()
            .map(|e| e.id.as_str())
            .collect();
        assert_eq!(
            ids,
            [
                "org.example.DbusOnly",
                "env-browser",
                "escaped",
                "firefox",
                "google-chrome",
                "term"
            ]
        );
    }

    /// The ids `lists`, each a list's text in precedence order, give as link candidates for https.
    fn candidates_from(lists: &[&str]) -> Vec<String> {
        let tmp = tempfile::tempdir().unwrap();
        let paths: Vec<PathBuf> = lists
            .iter()
            .enumerate()
            .map(|(at, text)| {
                let path = tmp.path().join(format!("{at}.list"));
                std::fs::write(&path, text).unwrap();
                path
            })
            .collect();
        registry()
            .link_candidates("x-scheme-handler/https", &MimeLists::load(&paths))
            .iter()
            .map(|entry| entry.id.clone())
            .collect()
    }

    #[test]
    fn a_lower_lists_removal_never_hides_what_a_higher_list_added() {
        let ids = candidates_from(&[
            "[Added Associations]\nx-scheme-handler/https=firefox.desktop;\n",
            "[Removed Associations]\nx-scheme-handler/https=firefox.desktop;\n",
        ]);
        assert_eq!(ids.first().map(String::as_str), Some("firefox"), "{ids:?}");
    }

    #[test]
    fn every_lists_additions_count_in_precedence_order() {
        let ids = candidates_from(&[
            "[Added Associations]\nx-scheme-handler/https=firefox.desktop;\n",
            "[Added Associations]\nx-scheme-handler/https=google-chrome.desktop;\n",
        ]);
        assert_eq!(ids[..2], ["firefox", "google-chrome"], "{ids:?}");
    }

    #[test]
    fn a_higher_lists_removal_hides_a_lower_lists_addition_and_a_declaration() {
        let ids = candidates_from(&[
            "[Removed Associations]\nx-scheme-handler/https=firefox.desktop;\n",
            "[Added Associations]\nx-scheme-handler/https=firefox.desktop;\n",
        ]);
        assert!(!ids.iter().any(|id| id == "firefox"), "{ids:?}");
    }

    #[test]
    fn an_escaped_working_folder_is_unescaped() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("folder.desktop"),
            "[Desktop Entry]\nType=Application\nName=Folder\nExec=x %u\nPath=/home/u/My\\sDir\n",
        )
        .unwrap();
        let r = Registry::load(&[tmp.path().to_owned()], &[]);
        assert_eq!(
            r.get("folder").and_then(|entry| entry.work_dir.as_deref()),
            Some(Path::new("/home/u/My Dir"))
        );
    }

    #[test]
    fn folders_linked_many_ways_at_every_level_load_in_bounded_time() {
        let tmp = tempfile::tempdir().unwrap();
        let levels = 8;
        for level in 0..levels {
            let here = tmp.path().join(format!("l{level}"));
            std::fs::create_dir_all(&here).unwrap();
            for link in 0..6 {
                std::os::unix::fs::symlink(
                    tmp.path().join(format!("l{}", level + 1)),
                    here.join(format!("to{link}")),
                )
                .unwrap();
            }
        }
        write_entry(&tmp.path().join(format!("l{levels}/deep.desktop")), "Deep");
        let root = tmp.path().join("l0");
        let found =
            crate::test_support::finishes(move || Registry::load(&[root], &[]).entries().len());
        assert!(found.is_some_and(|count| count > 0), "{found:?}");
    }
}
