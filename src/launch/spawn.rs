use std::ffi::OsStr;
use std::future::Future;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant};

use super::exec::{ExecError, tokenize};

pub const EARLY_FAILURE_WINDOW: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    pub argv: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PrecheckError {
    #[error("empty command")]
    Empty,
    #[error("`{0}` was not found on PATH")]
    NotOnPath(String),
    #[error("TryExec `{0}` is not available")]
    TryExec(String),
    #[error("working directory `{}` does not exist", .0.display())]
    BadWorkingDir(PathBuf),
    /// The host could not be asked, or gave no answer that names one of the others.
    #[error("the command could not be checked on the host: {0}")]
    Host(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EarlyFailure {
    pub program: String,
    pub code: Option<i32>,
}

fn is_executable_file(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// Resolve `program` to an executable file: a path when it contains `/`, otherwise the first match
/// in `path_var`.
#[must_use]
pub fn resolve_program(program: &str, path_var: &OsStr) -> Option<PathBuf> {
    if program.contains('/') {
        let p = PathBuf::from(program);
        return is_executable_file(&p).then_some(p);
    }
    std::env::split_paths(path_var)
        .map(|dir| dir.join(program))
        .find(|p| is_executable_file(p))
}

/// Check the command can start: non-empty argv, `TryExec` and the program resolvable, cwd present.
///
/// # Errors
/// The first failing [`PrecheckError`], checked in that order.
pub fn precheck(
    spec: &LaunchSpec,
    try_exec: Option<&str>,
    path_var: &OsStr,
) -> Result<(), PrecheckError> {
    let program = spec.argv.first().ok_or(PrecheckError::Empty)?;
    if let Some(try_exec) = try_exec
        && resolve_program(try_exec, path_var).is_none()
    {
        return Err(PrecheckError::TryExec(try_exec.to_owned()));
    }
    if resolve_program(program, path_var).is_none() {
        return Err(PrecheckError::NotOnPath(program.clone()));
    }
    if let Some(cwd) = &spec.cwd
        && !cwd.is_dir()
    {
        return Err(PrecheckError::BadWorkingDir(cwd.clone()));
    }
    Ok(())
}

/// Prefix `argv` with the tokenized `terminal` command and `-e`.
///
/// # Errors
/// The errors of [`tokenize`] for a malformed `terminal`.
pub fn wrap_in_terminal(argv: Vec<String>, terminal: &str) -> Result<Vec<String>, ExecError> {
    let mut out = tokenize(terminal)?;
    out.push("-e".to_owned());
    out.extend(argv);
    Ok(out)
}

/// The terminal configured in COSMIC settings, resolved like libcosmic's `spawn_desktop_exec`
/// (`desktop.rs:809`), falling back to `cosmic-term`.
#[must_use]
pub fn configured_terminal() -> String {
    cosmic_settings_config::shortcuts::context()
        .ok()
        .and_then(|config| {
            cosmic_settings_config::shortcuts::system_actions(&config)
                .get(&cosmic_settings_config::shortcuts::action::System::Terminal)
                .cloned()
        })
        .unwrap_or_else(|| String::from("cosmic-term"))
}

/// The shell that holds a program back. Its stdin is the gate: `read` returns when the gate is dropped
/// and its write end closes. `exec` then makes the program of the same pid, with stdin null.
const GATE_SHELL: &str = "/bin/sh";
const GATE_SCRIPT: &str = r#"read _; exec "$@" </dev/null"#;

/// What holds a program back from starting, and then watches how it begins.
pub struct Gate {
    writer: std::io::PipeWriter,
    program: String,
    window: Duration,
    exit: tokio::sync::oneshot::Receiver<(std::io::Result<ExitStatus>, Instant)>,
}

impl Gate {
    /// Lets the program start, and gives a future resolving to `Some` when it exits non-zero within the
    /// window of now (however late the future is polled). A child that died before the gate opened counts
    /// as having failed at once.
    pub fn open(self) -> impl Future<Output = Option<EarlyFailure>> + Send + 'static {
        let Self {
            writer,
            program,
            window,
            mut exit,
        } = self;
        drop(writer);
        let opened = Instant::now();
        let deadline = tokio::time::Instant::from_std(opened + window);
        async move {
            let exit = match tokio::time::timeout_at(deadline, &mut exit).await {
                Ok(received) => received.ok(),
                // Passing the deadline does not mean the exit is not queued; judge it by its timestamp.
                Err(_) => exit.try_recv().ok(),
            };
            let (Ok(status), exited_at) = exit? else {
                return None;
            };
            (!status.success() && exited_at.duration_since(opened) < window).then(|| EarlyFailure {
                program,
                code: status.code(),
            })
        }
    }
}

/// Spawn detached (own session via `setsid`, stdin null), with `XDG_ACTIVATION_TOKEN`/`DESKTOP_STARTUP_ID`
/// set only when a token exists. Returns the pid and the [`Gate`] that holds the program back. The child
/// is always reaped by a background thread.
///
/// The program does not start until the gate is opened, and then keeps the pid, so the caller can put
/// that pid in a cgroup of its own while nothing the program starts exists yet. Open it on every path;
/// dropping it opens it too, but nothing then watches the program's start.
///
/// # Errors
/// An empty argv, a reaper thread that cannot be created (nothing is launched), a program that is not
/// on the path, or the spawn failure (`setsid` failed).
pub fn spawn_detached(spec: &LaunchSpec, window: Duration) -> std::io::Result<(u32, Gate)> {
    spawn_detached_with(spec, window, |reaper| {
        std::thread::Builder::new()
            .name("signpost-reaper".to_owned())
            .spawn(reaper)
            .map(drop)
    })
}

/// [`spawn_detached`] with the reaper thread creation injected. `spawn_thread` must run the job it is
/// given; it is called before the child is launched so a failure there launches nothing.
fn spawn_detached_with(
    spec: &LaunchSpec,
    window: Duration,
    spawn_thread: impl FnOnce(Box<dyn FnOnce() + Send>) -> std::io::Result<()>,
) -> std::io::Result<(u32, Gate)> {
    let program = spec
        .argv
        .first()
        .cloned()
        .ok_or_else(|| std::io::Error::other("empty argv"))?;
    let (gate_reader, writer) = std::io::pipe()?;
    let mut cmd = Command::new(GATE_SHELL);
    cmd.args(["-c", GATE_SCRIPT, "sh"])
        .args(&spec.argv)
        .stdin(gate_reader);
    #[allow(unsafe_code)]
    // SAFETY: the closure runs in the forked child before exec and only calls setsid(2), which is
    // async-signal-safe; it allocates nothing and touches no state shared with the parent.
    unsafe {
        cmd.pre_exec(|| {
            rustix::process::setsid()
                .map(|_| ())
                .map_err(std::io::Error::from)
        });
    }
    if let Some(cwd) = &spec.cwd {
        cmd.current_dir(cwd);
    }
    match &spec.token {
        Some(token) => {
            cmd.env("XDG_ACTIVATION_TOKEN", token)
                .env("DESKTOP_STARTUP_ID", token);
        }
        None => {
            cmd.env_remove("XDG_ACTIVATION_TOKEN")
                .env_remove("DESKTOP_STARTUP_ID");
        }
    }
    // The reaper exists before the child does: it waits for the `Child`, so a thread-creation failure
    // launches nothing, and a failed launch drops `child_tx`, which ends the reaper.
    let (child_tx, child_rx) = std::sync::mpsc::channel::<Child>();
    let (exit_tx, exit_rx) = tokio::sync::oneshot::channel();
    spawn_thread(Box::new(move || {
        let Ok(mut child) = child_rx.recv() else {
            return;
        };
        let status = child.wait();
        let _ = exit_tx.send((status, Instant::now()));
    }))?;
    // The shell would only report a missing program once it is released, as an exit code.
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    resolve_program(&program, &path_var)
        .ok_or_else(|| std::io::Error::from(rustix::io::Errno::NOENT))?;
    let child = cmd.spawn()?;
    let pid = child.id();
    // The reaper holds `child_rx` until it receives, so this cannot fail while `spawn_thread` honours
    // its contract.
    let _ = child_tx.send(child);
    let gate = Gate {
        writer,
        program,
        window,
        exit: exit_rx,
    };
    Ok((pid, gate))
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::{Mutex, MutexGuard, PoisonError};

    use super::*;

    /// Held while a script is written and while any child is forked. A fork that overlaps a write
    /// inherits the still-open write descriptor until its exec, and exec'ing that script then fails
    /// with ETXTBSY. Held only across synchronous calls, never across an `.await`.
    static SCRIPT_LOCK: Mutex<()> = Mutex::new(());

    pub(crate) fn script_lock() -> MutexGuard<'static, ()> {
        SCRIPT_LOCK.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Shadows the glob-imported [`super::spawn_detached`] so every test launch takes the lock. The gate
    /// stays shut until the test opens or drops it.
    fn spawn_gated(spec: &LaunchSpec, window: Duration) -> std::io::Result<(u32, Gate)> {
        let _fork = script_lock();
        super::spawn_detached(spec, window)
    }

    /// A launch whose gate opens at once, for the tests that are not about it.
    fn spawn_detached(
        spec: &LaunchSpec,
        window: Duration,
    ) -> std::io::Result<(
        u32,
        impl Future<Output = Option<EarlyFailure>> + Send + 'static,
    )> {
        let (pid, gate) = spawn_gated(spec, window)?;
        Ok((pid, gate.open()))
    }

    /// Shadows the glob-imported [`super::spawn_detached_with`] for the same reason.
    fn spawn_detached_with(
        spec: &LaunchSpec,
        window: Duration,
        spawn_thread: impl FnOnce(Box<dyn FnOnce() + Send>) -> std::io::Result<()>,
    ) -> std::io::Result<(u32, Gate)> {
        let _fork = script_lock();
        super::spawn_detached_with(spec, window, spawn_thread)
    }

    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let _write = script_lock();
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    async fn wait_for(path: &Path) -> String {
        for _ in 0..100 {
            if let Ok(s) = std::fs::read_to_string(path)
                && s.ends_with("end\n")
            {
                return s;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("{} never completed", path.display());
    }

    const ECHO: &str = r#"out="$1"; shift
{ printf 'cwd=%s\n' "$PWD"; printf 'token=%s\n' "${XDG_ACTIVATION_TOKEN-<unset>}"; printf 'startup=%s\n' "${DESKTOP_STARTUP_ID-<unset>}"; for a in "$@"; do printf 'arg=%s\n' "$a"; done; echo end; } > "$out""#;

    #[test]
    fn resolve_program_searches_the_given_path_only() {
        let dir = tempfile::tempdir().unwrap();
        let exe = script(dir.path(), "tool", "exit 0");
        std::fs::write(dir.path().join("plain"), "x").unwrap();
        let path_var = dir.path().as_os_str();
        assert_eq!(resolve_program("tool", path_var), Some(exe.clone()));
        assert_eq!(
            resolve_program(exe.to_str().unwrap(), OsStr::new("")),
            Some(exe)
        );
        assert_eq!(resolve_program("plain", path_var), None);
        assert_eq!(resolve_program("tool", OsStr::new("/nonexistent")), None);
    }

    #[test]
    fn precheck_reports_each_failure() {
        let dir = tempfile::tempdir().unwrap();
        script(dir.path(), "tool", "exit 0");
        let path_var = dir.path().as_os_str();
        let ok = LaunchSpec {
            argv: vec!["tool".into()],
            cwd: Some(dir.path().into()),
            token: None,
        };
        assert_eq!(precheck(&ok, None, path_var), Ok(()));
        assert_eq!(
            precheck(
                &LaunchSpec {
                    argv: vec![],
                    ..ok.clone()
                },
                None,
                path_var
            ),
            Err(PrecheckError::Empty)
        );
        assert_eq!(
            precheck(
                &LaunchSpec {
                    argv: vec!["missing".into()],
                    ..ok.clone()
                },
                None,
                path_var
            ),
            Err(PrecheckError::NotOnPath("missing".into()))
        );
        assert_eq!(
            precheck(&ok, Some("not-here"), path_var),
            Err(PrecheckError::TryExec("not-here".into()))
        );
        let bad_cwd = dir.path().join("gone");
        assert_eq!(
            precheck(
                &LaunchSpec {
                    cwd: Some(bad_cwd.clone()),
                    ..ok
                },
                None,
                path_var
            ),
            Err(PrecheckError::BadWorkingDir(bad_cwd))
        );
    }

    #[test]
    fn terminal_wrapping_tokenizes_the_configured_terminal() {
        assert_eq!(
            wrap_in_terminal(vec!["htop".into()], "cosmic-term --profile 'Big Font'").unwrap(),
            ["cosmic-term", "--profile", "Big Font", "-e", "htop"]
        );
    }

    #[tokio::test]
    async fn spawn_passes_argv_cwd_and_token_exactly() {
        let dir = tempfile::tempdir().unwrap();
        let echo = script(dir.path(), "echo-args", ECHO);
        let out = dir.path().join("out.txt");
        let uri = "https://ex.org/a b/%20x?q='1'&r=\"2\"#frag";
        let spec = LaunchSpec {
            argv: vec![
                echo.to_string_lossy().into(),
                out.to_string_lossy().into(),
                uri.into(),
                "two words".into(),
            ],
            cwd: Some(dir.path().into()),
            token: Some("tok-1".into()),
        };
        let (_pid, early) = spawn_detached(&spec, Duration::from_millis(500)).unwrap();
        assert_eq!(early.await, None);
        let text = wait_for(&out).await;
        assert_eq!(
            text,
            format!(
                "cwd={}\ntoken=tok-1\nstartup=tok-1\narg={uri}\narg=two words\nend\n",
                dir.path().display()
            )
        );
    }

    /// Runs only inside the child re-executed by `inherited_token_variables_are_cleared`.
    #[tokio::test]
    #[ignore = "helper: executed in a child process with seeded token variables"]
    async fn inherited_token_child() {
        let out =
            PathBuf::from(std::env::var_os("SIGNPOST_TEST_OUT").expect("set by the parent test"));
        let echo = script(out.parent().unwrap(), "echo-args", ECHO);
        let spec = LaunchSpec {
            argv: vec![echo.to_string_lossy().into(), out.to_string_lossy().into()],
            cwd: None,
            token: None,
        };
        let (_pid, early) = spawn_detached(&spec, Duration::from_millis(500)).unwrap();
        assert_eq!(early.await, None);
        wait_for(&out).await;
    }

    #[test]
    fn inherited_token_variables_are_cleared() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out.txt");
        let mut child = std::process::Command::new(std::env::current_exe().unwrap());
        child
            .args([
                "--ignored",
                "--exact",
                "launch::spawn::tests::inherited_token_child",
                "--test-threads=1",
            ])
            .env("SIGNPOST_TEST_OUT", &out)
            .env("XDG_ACTIVATION_TOKEN", "inherited-token")
            .env("DESKTOP_STARTUP_ID", "inherited-id");
        let fork = script_lock();
        let mut child = child.spawn().unwrap();
        drop(fork);
        let status = child.wait().unwrap();
        assert!(status.success(), "child test failed");
        let text = std::fs::read_to_string(&out).unwrap();
        assert!(
            text.contains("token=<unset>\n") && text.contains("startup=<unset>\n"),
            "{text}"
        );
    }

    #[tokio::test]
    async fn child_runs_in_its_own_session() {
        let dir = tempfile::tempdir().unwrap();
        let sid = script(
            dir.path(),
            "sid",
            r#"out="$1"; { cut -d' ' -f6 /proc/$$/stat; echo end; } > "$out""#,
        );
        let out = dir.path().join("sid.txt");
        let spec = LaunchSpec {
            argv: vec![sid.to_string_lossy().into(), out.to_string_lossy().into()],
            cwd: None,
            token: None,
        };
        let (pid, early) = spawn_detached(&spec, Duration::from_millis(500)).unwrap();
        assert_eq!(early.await, None);
        let text = wait_for(&out).await;
        let child_sid: i32 = text.lines().next().unwrap().trim().parse().unwrap();
        let our_sid = rustix::process::getsid(None)
            .unwrap()
            .as_raw_nonzero()
            .get();
        assert_ne!(child_sid, our_sid, "child must not share our session");
        assert_eq!(
            child_sid,
            i32::try_from(pid).unwrap(),
            "the child leads its own new session"
        );
    }

    #[tokio::test]
    async fn the_program_runs_only_once_the_gate_opens_and_under_the_pid_spawn_returned() {
        let dir = tempfile::tempdir().unwrap();
        let forker = script(
            dir.path(),
            "forker",
            r#"d="$1"; echo $$ > "$d/pid"; readlink /proc/$$/fd/0 > "$d/stdin"; sleep 5 & echo $! > "$d/child"; echo end > "$d/done""#,
        );
        let spec = LaunchSpec {
            argv: vec![
                forker.to_string_lossy().into(),
                dir.path().to_string_lossy().into(),
            ],
            cwd: None,
            token: None,
        };
        let (pid, gate) = spawn_gated(&spec, Duration::from_millis(500)).unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            !dir.path().join("pid").exists(),
            "the program ran before the gate opened"
        );
        drop(gate);
        wait_for(&dir.path().join("done")).await;
        let recorded = |name: &str| std::fs::read_to_string(dir.path().join(name)).unwrap();
        assert_eq!(recorded("pid").trim().parse::<u32>().unwrap(), pid);
        assert_eq!(recorded("stdin"), "/dev/null\n", "stdin is not the gate");
        let child = recorded("child").trim().parse::<i32>().unwrap();
        rustix::process::kill_process(
            rustix::process::Pid::from_raw(child).unwrap(),
            rustix::process::Signal::KILL,
        )
        .unwrap();
    }

    /// How long the window of the gated tests is, and how much longer than it the gate is held.
    const WINDOW: Duration = Duration::from_secs(1);
    const HELD_PAST_WINDOW: Duration = Duration::from_millis(1200);

    #[tokio::test]
    async fn the_window_starts_when_the_gate_opens_and_a_death_before_it_still_counts() {
        let dir = tempfile::tempdir().unwrap();
        let fail = script(dir.path(), "fails-once-released", "exit 7");
        let spec = LaunchSpec {
            argv: vec![fail.to_string_lossy().into()],
            cwd: None,
            token: None,
        };
        let failure = |code| {
            Some(EarlyFailure {
                program: fail.to_string_lossy().into(),
                code,
            })
        };

        let (_pid, gate) = spawn_gated(&spec, WINDOW).unwrap();
        tokio::time::sleep(HELD_PAST_WINDOW).await;
        assert_eq!(gate.open().await, failure(Some(7)), "held past the window");

        let (pid, gate) = spawn_gated(&spec, WINDOW).unwrap();
        rustix::process::kill_process(
            rustix::process::Pid::from_raw(i32::try_from(pid).unwrap()).unwrap(),
            rustix::process::Signal::KILL,
        )
        .unwrap();
        tokio::time::sleep(HELD_PAST_WINDOW).await;
        assert_eq!(
            gate.open().await,
            failure(None),
            "killed before the gate opened"
        );
    }

    #[tokio::test]
    async fn early_nonzero_exit_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let fail = script(dir.path(), "wrapper-fails", "exit 7");
        let spec = LaunchSpec {
            argv: vec![fail.to_string_lossy().into()],
            cwd: None,
            token: None,
        };
        let (_pid, early) = spawn_detached(&spec, Duration::from_secs(2)).unwrap();
        assert_eq!(
            early.await,
            Some(EarlyFailure {
                program: fail.to_string_lossy().into(),
                code: Some(7)
            })
        );
    }

    #[test]
    fn missing_binary_is_a_spawn_error() {
        let spec = LaunchSpec {
            argv: vec!["/nonexistent/browser".into()],
            cwd: None,
            token: None,
        };
        assert!(spawn_detached(&spec, Duration::from_millis(10)).is_err());
    }

    #[tokio::test]
    async fn early_failure_is_reported_even_when_polled_late() {
        let dir = tempfile::tempdir().unwrap();
        let fail = script(dir.path(), "fails-fast", "exit 7");
        let spec = LaunchSpec {
            argv: vec![fail.to_string_lossy().into()],
            cwd: None,
            token: None,
        };
        let (_pid, early) = spawn_detached(&spec, Duration::from_secs(2)).unwrap();
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert_eq!(
            early.await,
            Some(EarlyFailure {
                program: fail.to_string_lossy().into(),
                code: Some(7)
            })
        );
    }

    #[tokio::test]
    async fn late_failure_is_not_reported_when_polled_late() {
        let dir = tempfile::tempdir().unwrap();
        let fail = script(dir.path(), "fails-slowly", "sleep 1; exit 7");
        let spec = LaunchSpec {
            argv: vec![fail.to_string_lossy().into()],
            cwd: None,
            token: None,
        };
        let (_pid, early) = spawn_detached(&spec, Duration::from_millis(300)).unwrap();
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert_eq!(early.await, None);
    }

    #[tokio::test]
    async fn long_running_child_returns_none_at_the_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let slow = script(dir.path(), "slow", "exec sleep 5");
        let spec = LaunchSpec {
            argv: vec![slow.to_string_lossy().into()],
            cwd: None,
            token: None,
        };
        let started = std::time::Instant::now();
        let (pid, early) = spawn_detached(&spec, Duration::from_millis(300)).unwrap();
        let result = early.await;
        let elapsed = started.elapsed();
        // Reaping continues after the watcher gave up: once killed, the child must leave no zombie.
        let raw = i32::try_from(pid).unwrap();
        rustix::process::kill_process(
            rustix::process::Pid::from_raw(raw).unwrap(),
            rustix::process::Signal::KILL,
        )
        .unwrap();
        assert_eq!(result, None);
        assert!(
            elapsed < Duration::from_secs(2),
            "returned after {elapsed:?}"
        );
        for _ in 0..100 {
            if !Path::new(&format!("/proc/{pid}")).exists() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("child {pid} was never reaped");
    }

    #[test]
    fn reaper_thread_failure_launches_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("marker");
        let touch = script(dir.path(), "touch-marker", r#"echo end > "$1""#);
        let spec = LaunchSpec {
            argv: vec![
                touch.to_string_lossy().into(),
                marker.to_string_lossy().into(),
            ],
            cwd: None,
            token: None,
        };
        let result = spawn_detached_with(&spec, Duration::from_millis(10), |_job| {
            Err(std::io::Error::other("cannot create a thread"))
        });
        assert!(
            result.err().is_some(),
            "thread creation failure must be an error"
        );
        std::thread::sleep(Duration::from_millis(300));
        assert!(!marker.exists(), "the child must never have been launched");
    }

    #[test]
    fn failed_launch_releases_the_reaper() {
        let reaper = std::sync::Arc::new(std::sync::Mutex::new(None));
        let capture = std::sync::Arc::clone(&reaper);
        let spec = LaunchSpec {
            argv: vec!["/nonexistent/browser".into()],
            cwd: None,
            token: None,
        };
        let result = spawn_detached_with(&spec, Duration::from_millis(10), move |job| {
            *capture.lock().unwrap() = Some(std::thread::spawn(job));
            Ok(())
        });
        assert!(result.err().is_some());
        let handle = reaper
            .lock()
            .unwrap()
            .take()
            .expect("the reaper is created before the child");
        for _ in 0..100 {
            if handle.is_finished() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("the reaper never exited after the launch failed");
    }
}
