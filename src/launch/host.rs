//! A launch from a Flatpak sandbox: the app runs on the host, through `flatpak-spawn --host`, in a transient systemd
//! scope of its own when the host can make one.

use std::process::Command;
use std::time::Duration;

use super::spawn::{LaunchSpec, PrecheckError};

/// The launch `spec` run on the host. Its working folder goes to the host as `--directory`, and its activation token
/// as the app's own environment, or unset there when it has none; the returned spec has neither, as they belong to
/// the app, not to the `flatpak-spawn` that starts it. With `scopes`, the app runs in the scope `unit`, described as
/// `description`, with `--expand-environment=no` so a `$` in a link stays as it is.
#[must_use]
pub fn on_host(spec: &LaunchSpec, unit: &str, description: &str, scopes: bool) -> LaunchSpec {
    let mut argv = vec!["flatpak-spawn".to_owned(), "--host".to_owned()];
    if let Some(cwd) = &spec.cwd {
        argv.push(format!("--directory={}", cwd.display()));
    }
    match &spec.token {
        Some(token) => argv.extend([
            format!("--env=XDG_ACTIVATION_TOKEN={token}"),
            format!("--env=DESKTOP_STARTUP_ID={token}"),
        ]),
        None => argv.extend([
            "--unset-env=XDG_ACTIVATION_TOKEN".to_owned(),
            "--unset-env=DESKTOP_STARTUP_ID".to_owned(),
        ]),
    }
    argv.push("--".to_owned());
    if scopes {
        argv.extend(
            [
                "systemd-run",
                "--user",
                "--scope",
                "--quiet",
                "--collect",
                "--expand-environment=no",
            ]
            .map(str::to_owned),
        );
        argv.push(format!("--unit={unit}"));
        argv.push(format!("--description={description}"));
        argv.push("--".to_owned());
    }
    argv.extend(spec.argv.iter().cloned());
    LaunchSpec {
        argv,
        cwd: None,
        token: None,
    }
}

/// The scope a launch of `desktop_id` from a Flatpak runs in: [`super::scope::unit_name`] with `suffix` in 8 hex
/// digits.
#[must_use]
pub fn scope_unit(desktop_id: &str, suffix: u32) -> String {
    super::scope::unit_name(desktop_id, &format!("{suffix:08x}"))
}

/// What [`super::spawn::precheck`] checks, checked on the host through `run` within `timeout`.
///
/// # Errors
/// [`PrecheckError::Empty`] before the host is asked, then the first check that fails there, or
/// [`PrecheckError::Host`] when the host cannot be asked or does not answer in time.
pub async fn precheck(
    spec: &LaunchSpec,
    try_exec: Option<&str>,
    run: impl Fn(&[&str]) -> Command,
    timeout: Duration,
) -> Result<(), PrecheckError> {
    let program = spec.argv.first().ok_or(PrecheckError::Empty)?;
    // `+` marks a value, so an empty one stays apart from none.
    let given = |value: Option<&str>| value.map(|value| format!("+{value}")).unwrap_or_default();
    let cwd = spec.cwd.as_ref().map(|cwd| cwd.to_string_lossy());
    let (try_exec_arg, cwd_arg) = (given(try_exec), given(cwd.as_deref()));
    let command = run(&["sh", "-c", PRECHECK, "sh", program, &try_exec_arg, &cwd_arg]);
    let answer = tokio::task::spawn_blocking(move || crate::host::ask(command, timeout))
        .await
        .unwrap_or(crate::host::Answer::Unstartable);
    let failed = match answer {
        crate::host::Answer::Exited {
            success: true,
            stdout,
        } => match stdout.as_slice() {
            b"ok\n" => return Ok(()),
            b"try-exec\n" => {
                return Err(PrecheckError::TryExec(
                    try_exec.unwrap_or_default().to_owned(),
                ));
            }
            b"path\n" => return Err(PrecheckError::NotOnPath(program.clone())),
            b"cwd\n" => {
                return Err(PrecheckError::BadWorkingDir(
                    spec.cwd.clone().unwrap_or_default(),
                ));
            }
            _ => "it gave an answer Signpost does not know",
        },
        crate::host::Answer::Exited { .. } => "the check failed there",
        crate::host::Answer::TimedOut => "it did not answer in time",
        crate::host::Answer::Unstartable => "it could not be asked",
    };
    Err(PrecheckError::Host(failed.to_owned()))
}

/// The checks of [`super::spawn::precheck`] in its order, as the host's `sh` runs them: `$1` is the program, `$2`
/// the `TryExec` and `$3` the working folder, each `+` and its value, or empty when there is none. It prints the
/// first check that fails, or `ok`. `PATH` is split on `:` alone and never globbed; an empty entry, the last one
/// included, is the working folder, where the host starts the app, or without one the folder the check runs in.
const PRECHECK: &str = r#"found() {
  case $1 in */*) [ -f "$1" ] && [ -x "$1" ]; return ;; esac
  path=$PATH
  case $path in '') path=. ;; *:) path=$path. ;; esac
  set -f; IFS=:
  for dir in $path; do
    [ -f "${dir:-.}/$1" ] && [ -x "${dir:-.}/$1" ] && return 0
  done
  return 1
}
case $3 in +*) if [ -d "${3#+}" ]; then cd "${3#+}" 2>/dev/null; else nodir=1; fi ;; esac
case $2 in +*) found "${2#+}" || { echo try-exec; exit; } ;; esac
found "$1" || { echo path; exit; }
[ -z "${nodir-}" ] || { echo cwd; exit; }
echo ok"#;

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn spec(token: Option<&str>, cwd: Option<&str>) -> LaunchSpec {
        LaunchSpec {
            argv: ["firefox", "--new-window", "https://x/?a=$HOME"]
                .map(String::from)
                .to_vec(),
            cwd: cwd.map(PathBuf::from),
            token: token.map(str::to_owned),
        }
    }

    #[test]
    fn a_launch_runs_on_the_host_in_its_scope_with_its_token_and_folder() {
        let wrapped = on_host(
            &spec(Some("tok"), Some("/home/u/work")),
            "app-signpost-firefox-0000002a.scope",
            "Firefox launched by Signpost",
            true,
        );
        assert_eq!(
            wrapped.argv,
            [
                "flatpak-spawn",
                "--host",
                "--directory=/home/u/work",
                "--env=XDG_ACTIVATION_TOKEN=tok",
                "--env=DESKTOP_STARTUP_ID=tok",
                "--",
                "systemd-run",
                "--user",
                "--scope",
                "--quiet",
                "--collect",
                "--expand-environment=no",
                "--unit=app-signpost-firefox-0000002a.scope",
                "--description=Firefox launched by Signpost",
                "--",
                "firefox",
                "--new-window",
                "https://x/?a=$HOME",
            ]
        );
        assert_eq!(wrapped.cwd, None, "the folder is the app's, on the host");
        assert_eq!(wrapped.token, None, "and so is the token");
    }

    #[test]
    fn without_a_token_folder_or_scope_the_host_launch_says_so() {
        let wrapped = on_host(
            &spec(None, None),
            "app-signpost-firefox-0000002a.scope",
            "x",
            false,
        );
        assert_eq!(
            wrapped.argv,
            [
                "flatpak-spawn",
                "--host",
                "--unset-env=XDG_ACTIVATION_TOKEN",
                "--unset-env=DESKTOP_STARTUP_ID",
                "--",
                "firefox",
                "--new-window",
                "https://x/?a=$HOME",
            ]
        );
    }

    #[test]
    fn a_scope_is_named_for_its_app_as_systemd_escapes_it() {
        assert_eq!(
            scope_unit("org.mozilla.firefox", 42),
            "app-signpost-org.mozilla.firefox-0000002a.scope"
        );
        assert_eq!(
            scope_unit("google-chrome", 1),
            "app-signpost-google\\x2dchrome-00000001.scope"
        );
        assert_eq!(scope_unit("a b", 1), "app-signpost-a\\x20b-00000001.scope");
        assert_eq!(
            scope_unit(".hidden", 1),
            "app-signpost-\\x2ehidden-00000001.scope"
        );
        let long = "x".repeat(300);
        let unit = scope_unit(&long, 7);
        assert!(unit.len() <= 255, "{} bytes", unit.len());
        let hash = unit
            .strip_prefix("app-signpost-")
            .and_then(|rest| rest.strip_suffix("-00000007.scope"))
            .expect("the hashed form");
        assert!(
            hash.len() == 16 && hash.chars().all(|c| c.is_ascii_hexdigit()),
            "{unit}"
        );
        assert_eq!(scope_unit(&long, 7), unit, "the same id, the same name");
    }

    /// A host that is this machine: the `sh` command `args` run with `path` as `PATH`, in `cwd`.
    fn here(path: &std::ffi::OsStr, cwd: &std::path::Path) -> impl Fn(&[&str]) -> Command {
        let (path, cwd) = (path.to_owned(), cwd.to_owned());
        move |args| {
            assert_eq!(args[0], "sh");
            // Named in full, as `path` is not where this machine keeps it.
            let mut command = Command::new("/bin/sh");
            command
                .args(&args[1..])
                .env("PATH", &path)
                .current_dir(&cwd);
            command
        }
    }

    fn checked<T>(future: impl std::future::Future<Output = T>) -> T {
        let _fork = crate::launch::spawn::tests::script_lock();
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(future)
    }

    #[test]
    fn the_host_precheck_finds_what_the_native_one_finds() {
        use std::os::unix::fs::PermissionsExt as _;
        let root = tempfile::tempdir().unwrap();
        let put = |dir: &str, name: &str, mode: u32| {
            let file = root.path().join(dir).join(name);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, "").unwrap();
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(mode)).unwrap();
            file.to_str().unwrap().to_owned()
        };
        let app = put("my bin", "app", 0o755);
        let plain = put("my bin", "plain", 0o644);
        put("decoy-glob", "globbed", 0o755);
        std::fs::create_dir(root.path().join("work")).unwrap();
        let work = root.path().join("work").to_str().unwrap().to_owned();
        let missing = root.path().join("gone").to_str().unwrap().to_owned();
        let path =
            std::env::join_paths([root.path().join("my bin"), root.path().join("decoy*")]).unwrap();
        let cases: &[(&str, Option<&str>, Option<&str>)] = &[
            ("app", None, None),
            ("absent", None, None),
            ("plain", None, None),
            ("globbed", None, None),
            (&app, None, None),
            (&plain, None, None),
            ("app", Some("absent"), None),
            ("app", Some("app"), None),
            ("app", Some(""), None),
            ("app", None, Some(&work)),
            ("app", None, Some(&missing)),
            ("app", None, Some("")),
            ("absent", Some("absent"), Some(&missing)),
            ("absent", None, Some(&missing)),
            ("$(touch hit)`touch tick`;touch semi", None, None),
        ];
        let (mut host, mut native) = (Vec::new(), Vec::new());
        for &(program, try_exec, cwd) in cases {
            let spec = LaunchSpec {
                argv: vec![program.to_owned(), "https://x".to_owned()],
                cwd: cwd.map(PathBuf::from),
                token: None,
            };
            host.push(checked(precheck(
                &spec,
                try_exec,
                here(&path, root.path()),
                crate::host::ASK_TIMEOUT,
            )));
            native.push(crate::launch::spawn::precheck(&spec, try_exec, &path));
        }
        assert_eq!(host, native);
        assert!(
            native.contains(&Ok(())) && native.iter().filter(|r| r.is_err()).count() > 5,
            "the cases cover both answers: {native:?}"
        );
        for made in ["hit", "tick", "semi"] {
            assert!(
                !root.path().join(made).exists(),
                "the program name ran: {made}"
            );
        }
    }

    #[test]
    fn an_empty_path_entry_is_the_folder_the_check_runs_in() {
        use std::os::unix::fs::PermissionsExt as _;
        let root = tempfile::tempdir().unwrap();
        let tool = root.path().join("tool");
        std::fs::write(&tool, "").unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        let spec = LaunchSpec {
            argv: vec!["tool".to_owned()],
            cwd: None,
            token: None,
        };
        let nowhere = root.path().join("nowhere");
        for path in [format!("{}:", nowhere.display()), String::new()] {
            let checked = checked(precheck(
                &spec,
                None,
                here(std::ffi::OsStr::new(&path), root.path()),
                crate::host::ASK_TIMEOUT,
            ));
            assert_eq!(checked, Ok(()), "PATH={path:?}");
        }
    }

    /// The host runs the app in its own folder, so the check resolves a relative PATH entry from there, not from the
    /// folder the question runs in.
    #[test]
    fn an_empty_path_entry_is_the_folder_the_app_starts_in() {
        use std::os::unix::fs::PermissionsExt as _;
        let (launch, asked) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let tool = launch.path().join("tool");
        std::fs::write(&tool, "").unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        let spec = LaunchSpec {
            argv: vec!["tool".to_owned()],
            cwd: Some(launch.path().to_owned()),
            token: None,
        };
        let path = format!("{}:", asked.path().join("nowhere").display());
        let checked = checked(precheck(
            &spec,
            None,
            here(std::ffi::OsStr::new(&path), asked.path()),
            crate::host::ASK_TIMEOUT,
        ));
        assert_eq!(checked, Ok(()));
    }

    #[test]
    fn nothing_to_run_is_found_before_the_host_is_asked() {
        let spec = LaunchSpec {
            argv: Vec::new(),
            cwd: None,
            token: None,
        };
        let asked = std::cell::Cell::new(false);
        let run = |_: &[&str]| {
            asked.set(true);
            Command::new("true")
        };
        assert_eq!(
            checked(precheck(&spec, None, run, crate::host::ASK_TIMEOUT)),
            Err(PrecheckError::Empty)
        );
        assert!(!asked.get());
    }

    #[test]
    fn a_host_that_cannot_answer_fails_the_precheck() {
        let spec = spec(None, None);
        let host = |script: &'static str| {
            move |_: &[&str]| {
                let mut command = Command::new("sh");
                command.args(["-c", script]);
                command
            }
        };
        let started = std::time::Instant::now();
        for (what, answer) in [
            (
                "a timeout",
                checked(precheck(
                    &spec,
                    None,
                    host("sleep 5"),
                    Duration::from_millis(200),
                )),
            ),
            (
                "a failure",
                checked(precheck(
                    &spec,
                    None,
                    host("echo ok; exit 3"),
                    crate::host::ASK_TIMEOUT,
                )),
            ),
            (
                "an unknown word",
                checked(precheck(
                    &spec,
                    None,
                    host("echo maybe"),
                    crate::host::ASK_TIMEOUT,
                )),
            ),
            (
                "no answer",
                checked(precheck(&spec, None, host(":"), crate::host::ASK_TIMEOUT)),
            ),
            (
                "no flatpak-spawn",
                checked(precheck(
                    &spec,
                    None,
                    |_: &[&str]| Command::new("/nonexistent/flatpak-spawn"),
                    crate::host::ASK_TIMEOUT,
                )),
            ),
        ] {
            assert!(
                matches!(answer, Err(PrecheckError::Host(_))),
                "{what}: {answer:?}"
            );
        }
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "the timeout held"
        );
    }
}
