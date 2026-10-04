//! The user's session as Signpost sees it. Run natively, that is the process's own environment and files; run as a
//! Flatpak, it is the host's, read through the sandbox's mounts.

use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Where Signpost runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Host {
    /// On the host itself: every path is used as the process sees it.
    Native,
    /// In a Flatpak sandbox: host paths come from [`HostEnv`] and are read through its views.
    Flatpak(HostEnv),
}

impl Host {
    /// The host path a path the process reads stands for, for the user to see.
    #[must_use]
    pub fn shown(&self, read: &Path) -> PathBuf {
        match self {
            Self::Native => read.to_owned(),
            Self::Flatpak(env) => env.shown(read),
        }
    }
}

mod report;
pub use report::report;

/// The file a Flatpak sandbox marks itself with.
pub const FLATPAK_INFO: &str = "/.flatpak-info";

/// Whether the process runs in a Flatpak sandbox, which marks itself with the file `info`.
#[must_use]
pub fn sandboxed(info: &Path) -> bool {
    info.exists()
}

/// The Flatpak browsers whose data, `~/.var/app/<id>`, the bundle is granted to read for their profiles.
pub const GRANTED_APP_DATA: [&str; 10] = [
    "com.google.Chrome",
    "org.chromium.Chromium",
    "com.brave.Browser",
    "com.brave.Origin",
    "com.vivaldi.Vivaldi",
    "com.microsoft.Edge",
    "org.mozilla.firefox",
    "io.gitlab.librewolf-community",
    "app.zen_browser.zen",
    "one.ablaze.floorp",
];

/// The host's environment, as far as Signpost needs it, and where the sandbox shows the host's files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostEnv {
    pub home: PathBuf,
    pub config_home: PathBuf,
    pub config_dirs: Vec<PathBuf>,
    pub data_home: PathBuf,
    pub data_dirs: Vec<PathBuf>,
    pub current_desktop: Option<String>,
    /// Whether launches can go into transient systemd scopes on the host.
    pub scopes: bool,
    /// How asking for the host's environment failed, when these paths are the defaults instead.
    pub unread: Option<Unread>,
    mounts: Mounts,
}

/// How asking the host for its environment failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Unread {
    #[error("timed out")]
    TimedOut,
    #[error("could not start")]
    Unstartable,
    #[error("failed")]
    Failed,
}

/// Where the sandbox shows host files: the host's OS trees under `os`, and everything else it shows under `rest`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Mounts {
    os: PathBuf,
    rest: PathBuf,
}

impl Default for Mounts {
    fn default() -> Self {
        Self {
            os: PathBuf::from("/run/host"),
            rest: PathBuf::from("/"),
        }
    }
}

/// The host trees the sandbox shows under `/run/host`: its own `/usr` and `/etc` are the runtime's.
const OS_TREES: [&str; 7] = ["/usr", "/etc", "/bin", "/sbin", "/lib", "/lib32", "/lib64"];
/// Host paths the sandbox has a version of its own of, or does not show.
const REFUSED: [&str; 7] = [
    "/tmp",
    "/run",
    "/proc",
    "/dev",
    "/sys",
    "/app",
    "/.flatpak-info",
];
/// The system's Flatpak installation, which the bundle is granted.
const FLATPAK_SYSTEM: &str = "/var/lib/flatpak";
/// Where an `OSTree` host keeps `/usr/local`, which links there; Flatpak shows it with the OS trees.
const USR_LOCAL: &str = "/var/usrlocal";
/// The parts of the host's `/var` the sandbox shows as the host's: the system's Flatpak installation, an `OSTree`
/// host's homes (its `/home` links to `/var/home`, and Flatpak follows the link) and its `/usr/local`. Flatpak gives
/// the sandbox private `/var/data`, `/var/config`, `/var/cache` and `/var/tmp`, and none of the rest.
const HOST_VAR: [&str; 3] = [FLATPAK_SYSTEM, "/var/home", USR_LOCAL];
/// What the sandbox shows at an anchor, a folder the walk starts below.
enum Anchor {
    /// A folder, or nothing yet: the walk goes on below it.
    Folder,
    /// A link, to this target in host coordinates.
    Link(PathBuf),
    /// Something the sandbox has a version of its own of, or an unreadable link.
    Refused,
}

/// A link's `target` at an anchor, joined to the anchor's `parent`: the `..` it starts with climb `parent` by name,
/// as the folders above an anchor are never looked at; the rest is left for the walk, link by link.
fn climb(parent: &Path, target: &Path) -> PathBuf {
    let mut base = parent.to_owned();
    let mut parts = target.components().peekable();
    while let Some(Component::ParentDir | Component::CurDir) = parts.peek() {
        if parts.next() == Some(Component::ParentDir) {
            base.pop();
        }
    }
    // An absolute target replaces `base` altogether.
    base.join(parts.collect::<PathBuf>())
}

/// How many links one path may pass through, as the kernel allows.
const MAX_LINKS: usize = 40;

/// Whether `path` names a folder by itself: absolute, and free of `..`, which only the host can resolve.
fn usable(path: &Path) -> bool {
    path.is_absolute() && !path.components().any(|part| part == Component::ParentDir)
}

/// `value` split on `:` into the usable paths it lists, or `default` when it lists none.
fn path_list(value: Option<&[u8]>, default: &[&str]) -> Vec<PathBuf> {
    use std::os::unix::ffi::OsStrExt as _;
    let listed: Vec<PathBuf> = value
        .unwrap_or_default()
        .split(|byte| *byte == b':')
        .map(|entry| PathBuf::from(std::ffi::OsStr::from_bytes(entry)))
        .filter(|path| usable(path))
        .collect();
    if listed.is_empty() {
        return default.iter().map(PathBuf::from).collect();
    }
    listed
}

impl HostEnv {
    /// The host environment as `env -0` prints it (`NAME=value`, each pair ending in a NUL), with the XDG defaults
    /// where a value is unset, empty or relative. `sandbox_home` stands in for an unusable `HOME`: Flatpak shows the
    /// user's home at the same path.
    #[must_use]
    pub fn parse(env0: &[u8], sandbox_home: &Path) -> Self {
        use std::os::unix::ffi::OsStrExt as _;
        // Paths keep their bytes: a host folder need not be named in UTF-8.
        let pairs: Vec<(&[u8], &[u8])> = env0
            .split(|byte| *byte == 0)
            .filter_map(|pair| {
                let at = pair.iter().position(|byte| *byte == b'=')?;
                Some((&pair[..at], &pair[at + 1..]))
            })
            .collect();
        let var = |name: &str| {
            pairs
                .iter()
                .find(|(key, _)| *key == name.as_bytes())
                .map(|(_, value)| *value)
                .filter(|value| !value.is_empty())
        };
        let absolute = |name: &str| {
            var(name)
                .map(|value| PathBuf::from(std::ffi::OsStr::from_bytes(value)))
                .filter(|path| usable(path))
        };
        let home = absolute("HOME").unwrap_or_else(|| sandbox_home.to_owned());
        Self {
            config_home: absolute("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config")),
            config_dirs: path_list(var("XDG_CONFIG_DIRS"), &["/etc/xdg"]),
            data_home: absolute("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local/share")),
            data_dirs: path_list(var("XDG_DATA_DIRS"), &["/usr/local/share", "/usr/share"]),
            current_desktop: var("XDG_CURRENT_DESKTOP")
                .map(|value| String::from_utf8_lossy(value).into_owned()),
            scopes: false,
            unread: None,
            mounts: Mounts::default(),
            home,
        }
    }

    /// The Flatpak browsers' data folders the bundle is granted.
    fn granted_app_data(&self) -> impl Iterator<Item = PathBuf> + '_ {
        let app_data = self.home.join(".var/app");
        GRANTED_APP_DATA.iter().map(move |id| app_data.join(id))
    }

    /// Where the sandbox can read the host path `path`, or `None` where the sandbox has a version of its own or none.
    #[must_use]
    pub fn readable(&self, path: &Path) -> Option<PathBuf> {
        if !usable(path) {
            return None;
        }
        let inside = path.strip_prefix("/").ok()?;
        let refused = REFUSED.iter().any(|refused| path.starts_with(refused))
            || (path.starts_with("/var") && !HOST_VAR.iter().any(|kept| path.starts_with(kept)))
            || (path.starts_with(self.home.join(".var/app"))
                && !self
                    .granted_app_data()
                    .any(|granted| path.starts_with(granted)));
        if refused {
            return None;
        }
        if OS_TREES.iter().any(|tree| path.starts_with(tree)) || path.starts_with(USR_LOCAL) {
            return Some(self.mounts.os.join(inside));
        }
        Some(self.mounts.rest.join(inside))
    }

    /// The deepest folder holding `path` that the sandbox shows as the host's: folders above it, such as `/var`
    /// above the system's Flatpak installation, are never looked at, as the sandbox shows them as its own.
    fn root_of(&self, path: &Path) -> PathBuf {
        let roots = OS_TREES
            .iter()
            .map(PathBuf::from)
            .chain(HOST_VAR.iter().map(PathBuf::from))
            .chain([self.home.clone()])
            .chain(self.granted_app_data());
        roots
            .filter(|root| path.starts_with(root))
            .max_by_key(|root| root.components().count())
            .unwrap_or_else(|| PathBuf::from("/"))
    }

    /// What the sandbox shows at the anchor `root`. `/` is always a folder.
    fn anchor_at(&self, root: &Path) -> Anchor {
        if root == Path::new("/") {
            return Anchor::Folder;
        }
        let Some(shown) = self.readable(root) else {
            return Anchor::Refused;
        };
        let is_link =
            std::fs::symlink_metadata(&shown).is_ok_and(|meta| meta.file_type().is_symlink());
        if !is_link {
            return Anchor::Folder;
        }
        std::fs::read_link(&shown).map_or(Anchor::Refused, Anchor::Link)
    }

    /// `path` with each symbolic link in it followed as the host would, or `None` when a link leads where
    /// [`HostEnv::readable`] refuses, or there are too many.
    #[must_use]
    pub fn resolve(&self, path: &Path) -> Option<PathBuf> {
        let mut path = path.to_owned();
        let mut links = 0;
        'walk: loop {
            // `..` may stand here, from a link's target: the walk resolves it, and checks every step it takes.
            if !path.is_absolute() {
                return None;
            }
            let root = self.root_of(&path);
            match self.anchor_at(&root) {
                Anchor::Refused => return None,
                Anchor::Folder => {}
                Anchor::Link(target) => {
                    links += 1;
                    if links > MAX_LINKS {
                        return None;
                    }
                    // The folders above an anchor are never looked at, so a relative target's `..` is taken by
                    // name.
                    let below = path.strip_prefix(&root).ok()?.to_owned();
                    path = climb(root.parent().unwrap_or(&root), &target).join(below);
                    continue 'walk;
                }
            }
            let below: Vec<Component> = path.strip_prefix(&root).ok()?.components().collect();
            let mut done = root;
            for (at, part) in below.iter().enumerate() {
                let rest: PathBuf = below[at + 1..].iter().collect();
                let next = match part {
                    Component::Normal(name) => done.join(name),
                    Component::ParentDir => {
                        // `done` holds no links, so its parent is the host's; the walk starts over from there.
                        path = done.parent().unwrap_or(&done).join(rest);
                        continue 'walk;
                    }
                    _ => continue,
                };
                let shown = self.readable(&next)?;
                match std::fs::symlink_metadata(&shown) {
                    Ok(meta) if meta.file_type().is_symlink() => {
                        links += 1;
                        if links > MAX_LINKS {
                            return None;
                        }
                        let target = std::fs::read_link(&shown).ok()?;
                        // An absolute target is a host path; a relative one hangs off the link's own folder.
                        path = done.join(target).join(rest);
                        continue 'walk;
                    }
                    Ok(_) => done = next,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        if rest.as_os_str().is_empty() {
                            return Some(next);
                        }
                        return Some(next.join(rest));
                    }
                    Err(_) => return None,
                }
            }
            return Some(done);
        }
    }
}

/// A host file: its path on the host, for anything shown to the user or passed to the host, and where the sandbox
/// reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostFile {
    pub path: PathBuf,
    pub read: PathBuf,
}

impl HostEnv {
    /// The `mimeapps.list` files that exist, in the order and by the names `cosmic_mime_apps::list_paths` reads them
    /// natively: the desktop's own list before `mimeapps.list`, in the config home, then each config dir, then each
    /// data dir's `applications`.
    #[must_use]
    pub fn mime_lists(&self) -> Vec<HostFile> {
        self.mime_list_paths()
            .iter()
            .filter_map(|path| self.existing(path))
            .collect()
    }

    /// Every host path [`HostEnv::mime_lists`] looks for a list at, in its order.
    #[must_use]
    pub fn mime_list_paths(&self) -> Vec<PathBuf> {
        let desktop_list = self.desktop_list();
        let dirs = std::iter::once(self.config_home.clone())
            .chain(self.config_dirs.iter().cloned())
            .chain(self.data_dirs.iter().map(|dir| dir.join("applications")));
        dirs.flat_map(|dir| {
            desktop_list
                .iter()
                .map(String::as_str)
                .chain([MIME_LIST])
                .map(move |name| dir.join(name))
                .collect::<Vec<_>>()
        })
        .collect()
    }

    /// The list setup writes, named as `cosmic_mime_apps::local_list_path` names it: the desktop's own list in the
    /// config home if it exists, `mimeapps.list` there otherwise; `None` where the sandbox cannot reach the host's,
    /// the desktop's own list included when it is there but out of reach.
    #[must_use]
    pub fn user_list(&self) -> Option<HostFile> {
        if let Some(name) = self.desktop_list() {
            let own = self.config_home.join(&name);
            if let Some(file) = self.existing(&own) {
                return Some(file);
            }
            // There, but out of reach: setup must not write the plain list in its place.
            let there = self
                .read_path(&self.config_home)
                .is_some_and(|dir| std::fs::symlink_metadata(dir.join(&name)).is_ok());
            if there {
                return None;
            }
        }
        let path = self.config_home.join(MIME_LIST);
        let read = self.readable(&self.resolve(&path)?)?;
        // A link to nothing is setup's to refuse, but the walk has followed it here: handed on, setup would make its
        // target instead.
        let linked = self.read_path(&self.config_home).is_some_and(|dir| {
            std::fs::symlink_metadata(dir.join(MIME_LIST))
                .is_ok_and(|meta| meta.file_type().is_symlink())
        });
        if linked && !read.exists() {
            return None;
        }
        Some(HostFile { path, read })
    }

    /// The desktop's own list, named as cosmic-mime-apps names it: the whole of `XDG_CURRENT_DESKTOP`, lowercased.
    fn desktop_list(&self) -> Option<String> {
        self.current_desktop
            .as_ref()
            .map(|desktop| format!("{}-{MIME_LIST}", desktop.to_ascii_lowercase()))
    }

    /// Where the sandbox reads the host path `path`, its links followed on the host's terms; `None` where the
    /// sandbox only has a version of its own.
    #[must_use]
    pub fn read_path(&self, path: &Path) -> Option<PathBuf> {
        self.readable(&self.resolve(path)?)
    }

    /// The host path a read path reads, for the user to see: the inverse of [`HostEnv::readable`]. A path that is
    /// not a read path is shown as it is.
    #[must_use]
    pub fn shown(&self, read: &Path) -> PathBuf {
        let host = |inside: &Path| Path::new("/").join(inside);
        if let Ok(inside) = read.strip_prefix(&self.mounts.os) {
            return host(inside);
        }
        read.strip_prefix(&self.mounts.rest)
            .map_or_else(|_| read.to_owned(), host)
    }

    /// This environment with its files shown under `root`: the host's OS trees under `root/os`, the rest under
    /// `root/rest`.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn mounted_under(mut self, root: &Path) -> Self {
        self.mounts = Mounts {
            os: root.join("os"),
            rest: root.join("rest"),
        };
        self
    }

    /// `path` if it is a regular file on the host, read where the sandbox shows it, with its links followed on the
    /// host's terms.
    #[must_use]
    pub fn existing(&self, path: &Path) -> Option<HostFile> {
        let read = self.read_path(path)?;
        read.is_file().then(|| HostFile {
            path: path.to_owned(),
            read,
        })
    }
}

/// The name of the lists of default apps.
const MIME_LIST: &str = "mimeapps.list";

impl crate::index::FsView for HostEnv {
    fn read_path(&self, path: &Path) -> Option<PathBuf> {
        HostEnv::read_path(self, path)
    }

    fn identity(&self, dir: &Path) -> Option<PathBuf> {
        self.resolve(dir)
    }
}

/// Where this process runs. In a Flatpak sandbox it asks the host about itself first, which takes at most twice
/// [`ASK_TIMEOUT`]; `SIGNPOST_HOST_SCOPES=off` in its environment leaves launches out of systemd scopes.
#[must_use]
pub fn detect() -> Host {
    if !sandboxed(Path::new(FLATPAK_INFO)) {
        return Host::Native;
    }
    let sandbox_home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let scopes_off = std::env::var("SIGNPOST_HOST_SCOPES").as_deref() == Ok("off");
    Host::Flatpak(HostEnv::discover(
        on_host,
        &sandbox_home,
        scopes_off,
        ASK_TIMEOUT,
    ))
}

/// How long start-up waits for each question it asks the host.
pub const ASK_TIMEOUT: Duration = Duration::from_secs(2);

/// What a command asked of the host gave back.
#[derive(Debug, PartialEq, Eq)]
pub enum Answer {
    Exited {
        success: bool,
        stdout: Vec<u8>,
    },
    /// It ran past its time and was stopped.
    TimedOut,
    Unstartable,
}

/// Runs `command`, stopping and reaping it once `timeout` passes.
#[must_use]
pub fn ask(mut command: Command, timeout: Duration) -> Answer {
    use std::io::Read as _;
    use std::os::unix::process::CommandExt as _;
    use std::process::Stdio;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        // A group of its own, so what it starts can be stopped with it.
        .process_group(0);
    let Ok(mut child) = command.spawn() else {
        return Answer::Unstartable;
    };
    let Some(mut out) = child.stdout.take() else {
        stop(&mut child);
        return Answer::Unstartable;
    };
    // Read as it comes, so a full pipe never stalls the command.
    let (sender, read) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut stdout = Vec::new();
        let _ = out.read_to_end(&mut stdout);
        let _ = sender.send(stdout);
    });
    let deadline = std::time::Instant::now() + timeout;
    let mut status = None;
    // The answer is in once the command has exited and its output closed, by the same deadline: something it
    // started may hold the output open after it exits.
    while std::time::Instant::now() < deadline {
        status = status.or_else(|| child.try_wait().ok().flatten());
        if let Some(status) = status
            && let Ok(stdout) = read.try_recv()
        {
            return Answer::Exited {
                success: status.success(),
                stdout,
            };
        }
        std::thread::sleep(POLL);
    }
    stop(&mut child);
    // With the whole group stopped, the output closes and the reader ends.
    let _ = read.recv_timeout(POLL * 10);
    Answer::TimedOut
}

/// Stops `child` and everything in its process group, and reaps it.
fn stop(child: &mut std::process::Child) {
    if let Some(group) = rustix::process::Pid::from_raw(child.id().cast_signed()) {
        let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// How often a question to the host is checked for its answer.
const POLL: Duration = Duration::from_millis(10);

/// A name for a transient unit no other run of Signpost shares: `<prefix>-<8 hex digits>.scope`.
#[must_use]
pub fn unit_name(prefix: &str) -> String {
    format!("{prefix}-{:08x}.scope", unit_suffix())
}

/// A number for a unit's name that no other unit of this or another run of Signpost is likely to have.
#[must_use]
pub fn unit_suffix() -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNT: AtomicU32 = AtomicU32::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.subsec_nanos());
    nanos ^ std::process::id().rotate_left(16) ^ COUNT.fetch_add(1, Ordering::Relaxed)
}

/// `flatpak-spawn --host` asking the host `args`, the way a Flatpak runs a command on the host: in the host's root,
/// so the sandbox's own folder never matters, and with `--watch-bus`, so the host ends the command when Signpost
/// stops waiting for it and kills `flatpak-spawn`, whose own signals cannot reach it.
#[must_use]
pub fn on_host(args: &[&str]) -> Command {
    let mut command = Command::new("flatpak-spawn");
    command
        .args(["--host", "--watch-bus", "--directory=/"])
        .args(args);
    command
}

impl HostEnv {
    /// The host's environment, read by running `env -0` through `on_host`, and whether the host can put launches in
    /// systemd scopes, proven by making one, unless `scopes_off`. Each question gets `timeout`; one that fails or
    /// times out gives the defaults, or no scopes.
    pub fn discover(
        on_host: impl Fn(&[&str]) -> Command,
        sandbox_home: &Path,
        scopes_off: bool,
        timeout: Duration,
    ) -> Self {
        let mut env = match ask(on_host(&["env", "-0"]), timeout) {
            Answer::Exited {
                success: true,
                stdout,
            } => Self::parse(&stdout, sandbox_home),
            answer => {
                // Never the output: the environment can hold secrets.
                let failure = match answer {
                    Answer::TimedOut => Unread::TimedOut,
                    Answer::Unstartable => Unread::Unstartable,
                    Answer::Exited { .. } => Unread::Failed,
                };
                tracing::warn!(
                    %failure,
                    "the host's environment could not be read; using the defaults"
                );
                Self {
                    unread: Some(failure),
                    ..Self::parse(b"", sandbox_home)
                }
            }
        };
        if scopes_off {
            return env;
        }
        let unit = format!("--unit={}", unit_name("app-signpost-probe"));
        let probe = [
            "systemd-run",
            "--user",
            "--scope",
            "--quiet",
            "--collect",
            "--expand-environment=no",
            unit.as_str(),
            "--",
            "true",
        ];
        env.scopes = matches!(
            ask(on_host(&probe), timeout),
            Answer::Exited { success: true, .. }
        );
        if !env.scopes {
            tracing::info!(
                "the host cannot put launches in systemd scopes; they start without one"
            );
        }
        env
    }
}

#[cfg(test)]
mod tests;
