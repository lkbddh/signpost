use std::os::unix::fs::symlink;

use super::*;

const HOME: &str = "/home/u";

/// A host environment whose sandbox shows the host's OS trees under `<root>/os` and the rest under `<root>/rest`.
fn host_in(root: &Path) -> HostEnv {
    let mut env = HostEnv::parse(b"HOME=/home/u\0", Path::new("/sandbox/home"));
    env.mounts = Mounts {
        os: root.join("os"),
        rest: root.join("rest"),
    };
    env
}

/// Writes `text` at the host path `path` as the sandbox shows it under `view`, the parent folders included.
fn put(view: &Path, path: &str, text: &str) {
    let file = view.join(path.trim_start_matches('/'));
    std::fs::create_dir_all(file.parent().expect("a parent")).expect("the folders");
    std::fs::write(file, text).expect("the file");
}

/// Makes the host path `path`, as the sandbox shows it under `view`, a link to `target`.
fn link(view: &Path, path: &str, target: &str) {
    let file = view.join(path.trim_start_matches('/'));
    std::fs::create_dir_all(file.parent().expect("a parent")).expect("the folders");
    symlink(target, file).expect("the link");
}

#[test]
fn the_host_environment_takes_absolute_values_and_defaults_the_rest() {
    let env0 =
        b"HOME=/home/u\0XDG_CONFIG_HOME=relative\0XDG_DATA_DIRS=/a::rel:/b\0XDG_CONFIG_DIRS=\0\
XDG_CURRENT_DESKTOP=COSMIC\0OTHER=x\ny\0";
    let env = HostEnv::parse(env0, Path::new("/sandbox/home"));
    assert_eq!(env.home, Path::new(HOME));
    assert_eq!(
        env.config_home,
        Path::new("/home/u/.config"),
        "a relative value is ignored"
    );
    assert_eq!(env.data_home, Path::new("/home/u/.local/share"));
    assert_eq!(env.data_dirs, [Path::new("/a"), Path::new("/b")]);
    assert_eq!(
        env.config_dirs,
        [Path::new("/etc/xdg")],
        "an empty value takes the default"
    );
    assert_eq!(env.current_desktop.as_deref(), Some("COSMIC"));
    assert!(!env.scopes, "scopes are proven by a probe, not assumed");

    let env = HostEnv::parse(
        b"XDG_DATA_DIRS=rel\0XDG_CURRENT_DESKTOP=\0",
        Path::new("/sandbox/home"),
    );
    assert_eq!(
        env.home,
        Path::new("/sandbox/home"),
        "no HOME: the sandbox's, the same folder"
    );
    assert_eq!(
        env.data_dirs,
        [Path::new("/usr/local/share"), Path::new("/usr/share")]
    );
    assert_eq!(env.current_desktop, None);
}

#[test]
fn the_sandbox_reads_host_os_trees_under_run_host_and_refuses_what_it_has_its_own_of() {
    let env = host_in(Path::new("/v"));
    let read = |path: &str| env.readable(Path::new(path));
    assert_eq!(
        read("/usr/share/a.desktop"),
        Some(PathBuf::from("/v/os/usr/share/a.desktop"))
    );
    assert_eq!(
        read("/etc/xdg/mimeapps.list"),
        Some(PathBuf::from("/v/os/etc/xdg/mimeapps.list"))
    );
    assert_eq!(
        read("/home/u/.config/x"),
        Some(PathBuf::from("/v/rest/home/u/.config/x"))
    );
    assert_eq!(
        read("/var/lib/flatpak/exports/share/applications/b.desktop"),
        Some(PathBuf::from(
            "/v/rest/var/lib/flatpak/exports/share/applications/b.desktop"
        ))
    );
    assert_eq!(
        read("/home/u/.var/app/com.google.Chrome/config/x"),
        Some(PathBuf::from(
            "/v/rest/home/u/.var/app/com.google.Chrome/config/x"
        ))
    );
    for refused in [
        "/tmp/x",
        "/var/data/x",
        "/var/config/x",
        "/var/tmp/x",
        "/var/log/x",
        "/run/user/1000/x",
        "/proc/1/environ",
        "/app/bin/signpost",
        "/home/u/.var/app/org.example.Other/x",
        "relative/x",
    ] {
        assert_eq!(read(refused), None, "{refused}");
    }
    assert_eq!(
        read("/usr2/x"),
        Some(PathBuf::from("/v/rest/usr2/x")),
        "a whole component only"
    );
}

#[test]
fn links_are_followed_in_host_coordinates_never_onto_a_sandbox_decoy() {
    let root = tempfile::tempdir().expect("a scratch folder");
    let env = host_in(root.path());
    let (os, rest) = (root.path().join("os"), root.path().join("rest"));
    put(&os, "/etc/xdg/x.desktop", "host");
    put(&rest, "/etc/xdg/x.desktop", "sandbox decoy");
    link(&rest, "/home/u/abs.desktop", "/etc/xdg/x.desktop");
    let resolved = env
        .resolve(Path::new("/home/u/abs.desktop"))
        .expect("resolved");
    assert_eq!(resolved, Path::new("/etc/xdg/x.desktop"));
    let read = env.readable(&resolved).expect("readable");
    assert_eq!(std::fs::read_to_string(read).expect("read"), "host");

    for (name, target) in [
        ("tmp", "/tmp/x"),
        ("data", "/var/data/x"),
        ("config", "/var/config/x"),
    ] {
        put(&rest, target, "sandbox decoy");
        link(&rest, &format!("/home/u/{name}.desktop"), target);
        assert_eq!(
            env.resolve(Path::new(&format!("/home/u/{name}.desktop"))),
            None,
            "{target}"
        );
    }

    put(&rest, "/home/shared/apps/a.desktop", "shared");
    link(&rest, "/home/u/apps", "../shared/apps");
    assert_eq!(
        env.resolve(Path::new("/home/u/apps/a.desktop")),
        Some(PathBuf::from("/home/shared/apps/a.desktop")),
        "a relative link to a folder"
    );

    link(&rest, "/home/u/loop-a", "loop-b");
    link(&rest, "/home/u/loop-b", "loop-a");
    assert_eq!(env.resolve(Path::new("/home/u/loop-a")), None, "a loop");

    assert_eq!(
        env.resolve(Path::new("/home/u/missing/x")),
        Some(PathBuf::from("/home/u/missing/x")),
        "a path that does not exist has no links to follow"
    );
}

#[test]
fn a_system_export_resolves_under_its_grant_while_the_rest_of_var_stays_refused() {
    let root = tempfile::tempdir().expect("a scratch folder");
    let env = host_in(root.path());
    let rest = root.path().join("rest");
    let exports = "/var/lib/flatpak/exports/share/applications";
    put(&rest, &format!("{exports}/direct.desktop"), "direct");
    put(
        &rest,
        "/var/lib/flatpak/app/org.example.App/current/active/export/share/applications/org.example.App.desktop",
        "linked",
    );
    link(
        &rest,
        &format!("{exports}/org.example.App.desktop"),
        "../../../app/org.example.App/current/active/export/share/applications/org.example.App.desktop",
    );
    assert_eq!(
        env.resolve(Path::new(&format!("{exports}/direct.desktop"))),
        Some(PathBuf::from(format!("{exports}/direct.desktop")))
    );
    let linked = env
        .resolve(Path::new(&format!("{exports}/org.example.App.desktop")))
        .expect("the linked export");
    assert_eq!(
        std::fs::read_to_string(env.readable(&linked).expect("readable")).expect("read"),
        "linked"
    );
    link(
        &rest,
        &format!("{exports}/escape.desktop"),
        "../../../../../data/x",
    );
    put(&rest, "/var/data/x", "sandbox decoy");
    assert_eq!(
        env.resolve(Path::new(&format!("{exports}/escape.desktop"))),
        None
    );
}

#[test]
fn a_sandbox_is_known_by_its_info_file() {
    let root = tempfile::tempdir().expect("a scratch folder");
    let info = root.path().join(".flatpak-info");
    assert!(!sandboxed(&info));
    std::fs::write(&info, "[Application]\n").expect("the info file");
    assert!(sandboxed(&info));
}

fn sh(script: &str) -> std::process::Command {
    let mut command = std::process::Command::new("sh");
    command.args(["-c", script]);
    command
}

#[test]
fn a_question_to_the_host_gets_its_answer_or_gives_up_in_time() {
    assert_eq!(
        ask(sh("printf 'a\\0b'"), ASK_TIMEOUT),
        Answer::Exited {
            success: true,
            stdout: b"a\0b".to_vec()
        }
    );
    assert!(matches!(
        ask(sh("exit 3"), ASK_TIMEOUT),
        Answer::Exited { success: false, .. }
    ));
    assert_eq!(
        ask(
            std::process::Command::new("/nonexistent/program"),
            ASK_TIMEOUT
        ),
        Answer::Unstartable
    );
    let started = std::time::Instant::now();
    assert_eq!(
        ask(sh("exec sleep 10"), std::time::Duration::from_millis(100)),
        Answer::TimedOut
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "it gave up after {:?}",
        started.elapsed()
    );
}

/// A stand-in for `flatpak-spawn --host`: runs `env` and the scope probe as the scripts given, and keeps the
/// arguments it was asked to run.
struct Stub {
    env: &'static str,
    probe: &'static str,
    asked: std::cell::RefCell<Vec<Vec<String>>>,
}

impl Stub {
    fn new(env: &'static str, probe: &'static str) -> Self {
        Self {
            env,
            probe,
            asked: std::cell::RefCell::default(),
        }
    }

    fn on_host(&self, args: &[&str]) -> std::process::Command {
        self.asked
            .borrow_mut()
            .push(args.iter().map(|arg| (*arg).to_owned()).collect());
        if args == ["env", "-0"] {
            return sh(self.env);
        }
        sh(self.probe)
    }
}

#[test]
fn start_up_reads_the_host_environment_and_proves_scopes_by_making_one() {
    let stub = Stub::new(
        "printf 'HOME=/home/h\\0XDG_CURRENT_DESKTOP=COSMIC\\0'",
        "exit 0",
    );
    let env = HostEnv::discover(
        |args| stub.on_host(args),
        Path::new("/sandbox/home"),
        false,
        ASK_TIMEOUT,
    );
    assert_eq!(env.home, Path::new("/home/h"));
    assert_eq!(env.unread, None);
    assert!(env.scopes);
    let asked = stub.asked.borrow();
    let probe = asked.last().expect("the scope probe");
    assert_eq!(
        probe[..6],
        [
            "systemd-run",
            "--user",
            "--scope",
            "--quiet",
            "--collect",
            "--expand-environment=no"
        ]
    );
    let suffix = probe[6]
        .strip_prefix("--unit=app-signpost-probe-")
        .and_then(|rest| rest.strip_suffix(".scope"));
    assert!(
        suffix.is_some_and(|hex| hex.len() == 8 && hex.chars().all(|c| c.is_ascii_hexdigit())),
        "{probe:?}"
    );
    assert_eq!(probe[7..], ["--", "true"]);
}

#[test]
fn start_up_falls_back_to_the_defaults_and_to_no_scopes() {
    for (env_script, probe, why, unread) in [
        ("exit 1", "exit 1", "both fail", Unread::Failed),
        (
            "exec sleep 10",
            "exec sleep 10",
            "both time out",
            Unread::TimedOut,
        ),
    ] {
        let stub = Stub::new(env_script, probe);
        let env = HostEnv::discover(
            |args| stub.on_host(args),
            Path::new("/sandbox/home"),
            false,
            std::time::Duration::from_millis(100),
        );
        assert_eq!(env.home, Path::new("/sandbox/home"), "{why}");
        assert_eq!(env.config_home, Path::new("/sandbox/home/.config"), "{why}");
        assert_eq!(env.unread, Some(unread), "{why}");
        assert!(!env.scopes, "{why}");
    }
}

#[test]
fn scopes_turned_off_are_never_probed() {
    let stub = Stub::new("printf 'HOME=/home/h\\0'", "exit 0");
    let env = HostEnv::discover(
        |args| stub.on_host(args),
        Path::new("/sandbox/home"),
        true,
        ASK_TIMEOUT,
    );
    assert!(!env.scopes);
    assert_eq!(
        stub.asked.borrow().len(),
        1,
        "only the environment was asked for"
    );
}

#[test]
fn a_home_that_is_itself_a_link_is_followed_on_the_host_and_one_seen_through_a_linked_parent_is_kept()
 {
    let root = tempfile::tempdir().expect("a scratch folder");
    let env = host_in(root.path());
    let (os, rest) = (root.path().join("os"), root.path().join("rest"));
    put(&os, "/etc/user-home/x", "host");
    put(&rest, "/etc/user-home/x", "sandbox decoy");
    std::fs::create_dir_all(rest.join("home")).expect("the folder");
    link(&rest, "/home/u", "/etc/user-home");
    let resolved = env.resolve(Path::new("/home/u/x")).expect("resolved");
    assert_eq!(resolved, Path::new("/etc/user-home/x"));
    assert_eq!(
        std::fs::read_to_string(env.readable(&resolved).expect("readable")).expect("read"),
        "host"
    );

    let root = tempfile::tempdir().expect("a scratch folder");
    let env = host_in(root.path());
    let rest = root.path().join("rest");
    std::fs::create_dir_all(rest.join("home")).expect("the folder");
    link(&rest, "/home/u", "/tmp/home");
    put(&rest, "/tmp/home/x", "sandbox decoy");
    assert_eq!(
        env.resolve(Path::new("/home/u/x")),
        None,
        "a home linked into /tmp"
    );

    // The sandbox shows `/home` as a link to `var/home`, as Silverblue's host has it: the home itself is a
    // folder there, so it stays the anchor and `/var` above it is never looked at.
    let root = tempfile::tempdir().expect("a scratch folder");
    let env = host_in(root.path());
    let rest = root.path().join("rest");
    put(&rest, "/var/home/u/x", "home");
    link(&rest, "/home", "var/home");
    let resolved = env.resolve(Path::new("/home/u/x")).expect("resolved");
    assert_eq!(resolved, Path::new("/home/u/x"));
    assert_eq!(
        std::fs::read_to_string(env.readable(&resolved).expect("readable")).expect("read"),
        "home"
    );
}

/// An `OSTree` host keeps its homes and `/usr/local` under `/var`: Flatpak follows `/home` to `/var/home`, and shows
/// `/var/usrlocal` with the OS trees, as `/usr/local` links there. The sandbox's own `/var` stays out.
#[test]
fn an_ostree_hosts_home_and_local_apps_under_var_are_read_where_flatpak_shows_them() {
    let root = tempfile::tempdir().expect("a scratch folder");
    let mut env = HostEnv::parse(b"HOME=/var/home/u\0", Path::new("/var/home/u"));
    env.mounts = Mounts {
        os: root.path().join("os"),
        rest: root.path().join("rest"),
    };
    let (os, rest) = (root.path().join("os"), root.path().join("rest"));
    put(&rest, "/var/home/u/.config/mimeapps.list", "home");
    put(&os, "/var/usrlocal/share/applications/a.desktop", "local");
    link(&os, "/usr/local", "../var/usrlocal");
    let read = |path: &str| {
        let resolved = env.resolve(Path::new(path)).expect(path);
        std::fs::read_to_string(env.readable(&resolved).expect(path)).expect(path)
    };
    assert_eq!(read("/var/home/u/.config/mimeapps.list"), "home");
    assert_eq!(read("/usr/local/share/applications/a.desktop"), "local");
    for refused in ["/var/data/x", "/var/tmp/x", "/var/log/x", "/var/homes/x"] {
        assert_eq!(env.readable(Path::new(refused)), None, "{refused}");
    }
}

#[test]
fn a_parent_component_never_slips_past_a_refusal() {
    let env = host_in(Path::new("/v"));
    for aliased in [
        "/home/u/../../tmp/x",
        "/home/u/.var/app/com.google.Chrome/../org.example.Other/x",
        "/var/lib/flatpak/../../data/x",
        "/usr/../tmp/x",
    ] {
        assert_eq!(env.readable(Path::new(aliased)), None, "{aliased}");
    }
    let env = HostEnv::parse(
        b"HOME=/home/u/../../tmp\0XDG_CONFIG_HOME=/home/u/../x\0",
        Path::new("/sandbox/home"),
    );
    assert_eq!(
        env.home,
        Path::new("/sandbox/home"),
        "a HOME with `..` is not used"
    );
    assert_eq!(env.config_home, Path::new("/sandbox/home/.config"));
}

#[test]
fn host_paths_keep_their_bytes() {
    use std::os::unix::ffi::OsStrExt as _;
    let env = HostEnv::parse(
        b"HOME=/home/\xff\0XDG_DATA_DIRS=/d\xfe:/e\0XDG_CURRENT_DESKTOP=CO\xffSMIC\0",
        Path::new("/sandbox/home"),
    );
    assert_eq!(env.home.as_os_str().as_bytes(), b"/home/\xff");
    assert_eq!(env.data_dirs[0].as_os_str().as_bytes(), b"/d\xfe");
    assert_eq!(env.data_dirs.len(), 2);
    assert_eq!(env.current_desktop.as_deref(), Some("CO\u{fffd}SMIC"));
}

/// The test process's own environment as `env -0` prints it.
fn own_env0() -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt as _;
    std::env::vars_os()
        .flat_map(|(name, value)| [name.as_bytes(), b"=", value.as_bytes(), b"\0"].concat())
        .collect()
}

/// Set in the child that runs the parity check, to the fixture tree its environment points into.
const PARITY: &str = "SIGNPOST_HOST_PARITY_FIXTURE";

#[test]
fn the_host_mime_lists_are_the_ones_cosmic_mime_apps_reads_for_the_same_environment() {
    if let Some(fixture) = std::env::var_os(PARITY) {
        let fixture = PathBuf::from(fixture);
        let mut env = HostEnv::parse(&own_env0(), &fixture.join("home"));
        // Seen from the host itself, every path reads where it is.
        env.mounts = Mounts {
            os: PathBuf::from("/"),
            rest: PathBuf::from("/"),
        };
        let ours: Vec<PathBuf> = env.mime_lists().into_iter().map(|file| file.path).collect();
        assert_eq!(ours, cosmic_mime_apps::list_paths());
        assert!(ours.len() >= 4, "the fixture's lists are found: {ours:?}");
        assert_eq!(
            env.user_list().map(|file| file.path),
            cosmic_mime_apps::local_list_path()
        );
        return;
    }
    // A child process whose home and XDG folders all lie in a fixture tree, so neither implementation reads
    // anyone's real folders. Not under /tmp, which the read view refuses.
    let beside_tests = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_owned))
        .expect("the test binary's folder");
    let fixture = tempfile::tempdir_in(beside_tests).expect("a fixture tree");
    let root = fixture.path();
    for list in [
        "home/.config/cosmic-mimeapps.list",
        "home/.config/mimeapps.list",
        "etc/xdg/mimeapps.list",
        "share/applications/mimeapps.list",
        "share2/applications/cosmic-mimeapps.list",
    ] {
        let path = root.join(list);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("the folders");
        std::fs::write(path, "[Default Applications]\n").expect("the list");
    }
    let joined = |dirs: &[&str]| {
        dirs.iter()
            .map(|dir| root.join(dir).display().to_string())
            .collect::<Vec<_>>()
            .join(":")
    };
    let output = std::process::Command::new(std::env::current_exe().expect("this test binary"))
        .args([
            "--exact",
            "host::tests::the_host_mime_lists_are_the_ones_cosmic_mime_apps_reads_for_the_same_environment",
        ])
        .env(PARITY, root)
        .env("HOME", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("home/.config"))
        .env("XDG_CONFIG_DIRS", joined(&["etc/xdg", "missing"]))
        .env("XDG_DATA_HOME", root.join("home/.local/share"))
        .env("XDG_DATA_DIRS", joined(&["share", "share2"]))
        .env("XDG_CURRENT_DESKTOP", "COSMIC")
        .output()
        .expect("the test binary runs");
    let report = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{report}");
    assert!(
        report.contains("1 passed"),
        "the parity check did not run:\n{report}"
    );
}

#[test]
fn the_host_mime_lists_come_in_order_from_where_the_sandbox_shows_the_hosts() {
    let root = tempfile::tempdir().expect("a scratch folder");
    let mut env = host_in(root.path());
    env.current_desktop = Some("COSMIC".to_owned());
    env.config_dirs = vec![PathBuf::from("/etc/xdg"), PathBuf::from("/missing")];
    env.data_dirs = vec![PathBuf::from("/usr/share")];
    let (os, rest) = (root.path().join("os"), root.path().join("rest"));
    put(&rest, "/home/u/.config/cosmic-mimeapps.list", "");
    put(&rest, "/home/u/.config/mimeapps.list", "");
    put(&os, "/etc/xdg/mimeapps.list", "");
    put(&rest, "/etc/xdg/cosmic-mimeapps.list", "sandbox decoy");
    put(&os, "/usr/share/applications/cosmic-mimeapps.list", "");
    let lists = env.mime_lists();
    let paths: Vec<&Path> = lists.iter().map(|file| file.path.as_path()).collect();
    assert_eq!(
        paths,
        [
            Path::new("/home/u/.config/cosmic-mimeapps.list"),
            Path::new("/home/u/.config/mimeapps.list"),
            Path::new("/etc/xdg/mimeapps.list"),
            Path::new("/usr/share/applications/cosmic-mimeapps.list"),
        ]
    );
    assert_eq!(
        lists[2].read,
        os.join("etc/xdg/mimeapps.list"),
        "read where the host's is shown"
    );
    assert_eq!(
        lists[0].read,
        rest.join("home/u/.config/cosmic-mimeapps.list")
    );
    assert_eq!(
        env.user_list().expect("the user list").path,
        Path::new("/home/u/.config/cosmic-mimeapps.list")
    );

    std::fs::remove_file(rest.join("home/u/.config/cosmic-mimeapps.list")).expect("removed");
    let user = env.user_list().expect("the user list");
    assert_eq!(user.path, Path::new("/home/u/.config/mimeapps.list"));
    assert_eq!(user.read, rest.join("home/u/.config/mimeapps.list"));

    env.config_home = PathBuf::from("/tmp/config");
    put(&rest, "/tmp/config/mimeapps.list", "sandbox decoy");
    assert_eq!(
        env.user_list(),
        None,
        "a config home the sandbox has its own of"
    );
}

#[test]
fn a_read_path_shows_as_the_host_path_it_reads() {
    let root = tempfile::tempdir().expect("a scratch folder");
    let env = host_in(root.path());
    let (os, rest) = (root.path().join("os"), root.path().join("rest"));
    assert_eq!(
        env.read_path(Path::new("/etc/xdg/x")),
        Some(os.join("etc/xdg/x"))
    );
    assert_eq!(env.read_path(Path::new("/tmp/x")), None);
    assert_eq!(env.shown(&os.join("etc/xdg/x")), Path::new("/etc/xdg/x"));
    assert_eq!(
        env.shown(&rest.join("home/u/.config/x")),
        Path::new("/home/u/.config/x")
    );
    assert_eq!(
        env.shown(Path::new("/elsewhere/x")),
        Path::new("/elsewhere/x"),
        "not a read path"
    );
    assert_eq!(
        Host::Native.shown(Path::new("/run/host/etc/x")),
        Path::new("/run/host/etc/x")
    );
    let production = HostEnv::parse(b"HOME=/home/u\0", Path::new("/sandbox/home"));
    assert_eq!(
        production.shown(Path::new("/run/host/etc/xdg/x")),
        Path::new("/etc/xdg/x")
    );
    assert_eq!(
        production.shown(Path::new("/home/u/x")),
        Path::new("/home/u/x")
    );
}

/// Whether the process `pid` is gone, reaped as well as stopped: a zombie still has its folder in /proc.
fn gone(pid: &str) -> bool {
    !Path::new("/proc").join(pid.trim()).exists()
}

#[test]
fn a_question_that_runs_out_of_time_is_stopped_and_reaped_with_whatever_it_started() {
    let root = tempfile::tempdir().expect("a scratch folder");
    let pid_file = root.path().join("pid");
    let script = format!("echo $$ > {}; exec sleep 10", pid_file.display());
    let started = std::time::Instant::now();
    assert_eq!(
        ask(sh(&script), std::time::Duration::from_millis(200)),
        Answer::TimedOut
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(1),
        "{:?}",
        started.elapsed()
    );
    let pid = std::fs::read_to_string(&pid_file).expect("the pid");
    assert!(gone(&pid), "the command {pid} was left running or unreaped");

    // The command exits at once, but what it started keeps its output open.
    let descendant = root.path().join("descendant");
    let script = format!("sleep 10 & echo $! > {}; exit 0", descendant.display());
    let started = std::time::Instant::now();
    assert_eq!(
        ask(sh(&script), std::time::Duration::from_millis(200)),
        Answer::TimedOut
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(1),
        "{:?}",
        started.elapsed()
    );
    let pid = std::fs::read_to_string(&descendant).expect("the pid");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !gone(&pid) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(
        gone(&pid),
        "what the command started, {pid}, was left running"
    );
}

#[test]
fn a_granted_browser_folder_that_is_a_link_is_followed_on_the_hosts_terms() {
    let root = tempfile::tempdir().expect("a scratch folder");
    let env = host_in(root.path());
    let rest = root.path().join("rest");
    put(
        &rest,
        "/home/u/.config/chrome/Default/Preferences",
        "profile",
    );
    link(
        &rest,
        "/home/u/.var/app/com.google.Chrome",
        "../../.config/chrome",
    );
    let resolved = env
        .resolve(Path::new(
            "/home/u/.var/app/com.google.Chrome/Default/Preferences",
        ))
        .expect("resolved");
    assert_eq!(
        resolved,
        Path::new("/home/u/.config/chrome/Default/Preferences")
    );
    assert_eq!(
        std::fs::read_to_string(env.readable(&resolved).expect("readable")).expect("read"),
        "profile"
    );

    link(
        &rest,
        "/home/u/.var/app/org.chromium.Chromium",
        "/tmp/chromium",
    );
    put(&rest, "/tmp/chromium/Default/Preferences", "sandbox decoy");
    assert_eq!(
        env.resolve(Path::new(
            "/home/u/.var/app/org.chromium.Chromium/Default/Preferences"
        )),
        None
    );
}

#[test]
fn an_anchor_links_target_is_walked_link_by_link_past_its_leading_climb() {
    let root = tempfile::tempdir().expect("a scratch folder");
    let env = host_in(root.path());
    let rest = root.path().join("rest");
    put(&rest, "/opt/chrome/Default/Preferences", "through the link");
    put(
        &rest,
        "/home/u/.config/chrome/Default/Preferences",
        "by name alone",
    );
    std::fs::create_dir_all(rest.join("home/u/.config")).expect("the folder");
    link(&rest, "/home/u/.config/link", "/opt/browser");
    std::fs::create_dir_all(rest.join("opt/browser")).expect("the folder");
    link(
        &rest,
        "/home/u/.var/app/com.google.Chrome",
        "../../.config/link/../chrome",
    );
    let resolved = env
        .resolve(Path::new(
            "/home/u/.var/app/com.google.Chrome/Default/Preferences",
        ))
        .expect("resolved");
    assert_eq!(resolved, Path::new("/opt/chrome/Default/Preferences"));

    link(&rest, "/home/u/.config/away", "/tmp/away");
    put(&rest, "/tmp/away/x", "sandbox decoy");
    link(
        &rest,
        "/home/u/.var/app/org.chromium.Chromium",
        "../../.config/away/../x",
    );
    assert_eq!(
        env.resolve(Path::new("/home/u/.var/app/org.chromium.Chromium/Default")),
        None,
        "a refused link in the target still refuses"
    );
}

#[test]
fn a_desktop_list_that_is_there_but_out_of_reach_is_never_swapped_for_the_plain_one() {
    let root = tempfile::tempdir().expect("a scratch folder");
    let mut env = host_in(root.path());
    env.current_desktop = Some("COSMIC".to_owned());
    let rest = root.path().join("rest");
    put(&rest, "/home/u/.config/mimeapps.list", "");
    link(
        &rest,
        "/home/u/.config/cosmic-mimeapps.list",
        "/tmp/lists/cosmic-mimeapps.list",
    );
    put(&rest, "/tmp/lists/cosmic-mimeapps.list", "sandbox decoy");
    assert_eq!(env.user_list(), None);
}

#[test]
fn a_user_list_that_links_to_nothing_is_never_handed_to_setup() {
    let root = tempfile::tempdir().expect("a scratch folder");
    let env = host_in(root.path());
    let rest = root.path().join("rest");
    std::fs::create_dir_all(rest.join("home/u/.config")).expect("the folder");
    link(
        &rest,
        "/home/u/.config/mimeapps.list",
        "/home/u/dotfiles/mimeapps.list",
    );
    assert_eq!(
        env.user_list(),
        None,
        "setup would create the missing target instead of refusing"
    );
    let left =
        std::fs::symlink_metadata(rest.join("home/u/.config/mimeapps.list")).expect("the link");
    assert!(left.file_type().is_symlink(), "the link is left as it was");
    assert!(
        !rest.join("home/u/dotfiles").exists(),
        "and its target is not made"
    );
}

#[test]
fn a_question_to_the_host_ends_with_signpost_and_runs_in_its_root() {
    let command = on_host(&["env", "-0"]);
    let args: Vec<String> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(command.get_program(), "flatpak-spawn");
    assert_eq!(
        args,
        ["--host", "--watch-bus", "--directory=/", "env", "-0"]
    );
}
