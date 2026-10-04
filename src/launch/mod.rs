pub mod dbus;
pub mod exec;
pub mod host;
pub mod scope;
pub mod spawn;

use crate::index::AppEntry;
use crate::profiles::{Env, Tile, TileAction, adapter_for, revalidate, same_store, user_data_dir};
use exec::{Context, ExecError, InsertError, expand_written, insert_args, launcher, takes_url};
use spawn::{LaunchSpec, wrap_in_terminal};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    Exec(LaunchSpec),
    DBus { desktop_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
    #[error(transparent)]
    Exec(#[from] ExecError),
    #[error(transparent)]
    Insert(#[from] InsertError),
    #[error("the profile “{0}” no longer exists")]
    StaleProfile(String),
    #[error("“{0}” has nothing to launch")]
    NoExec(String),
    #[error("action “{0}” is not available")]
    ActionMissing(String),
    #[error("action “{0}” uses a different profile store than this profile")]
    ConflictingRoot(String),
}

/// Build what to run. The token is filled in by the caller (it is requested from the live picker surface).
///
/// # Errors
/// [`PlanError::ActionMissing`] / [`PlanError::NoExec`] when there is nothing to run, [`PlanError::StaleProfile`]
/// when the tile's profile is gone, [`PlanError::ConflictingRoot`] when a desktop action would reach a
/// different profile store than the tile's profile, plus the Exec and insertion errors.
pub fn plan(
    entry: &AppEntry,
    tile: &Tile,
    action: Option<&TileAction>,
    uri: &str,
    env: &Env,
    terminal: &str,
) -> Result<Plan, PlanError> {
    let base: &[String] = match action {
        Some(TileAction::Desktop { id, .. }) => {
            &entry
                .actions
                .iter()
                .find(|a| &a.id == id)
                .ok_or_else(|| PlanError::ActionMissing(id.clone()))?
                .exec
        }
        _ => match &entry.exec {
            Some(argv) if takes_url(argv) || !entry.dbus_activatable => argv,
            // With no Exec, or one that would drop the link, the app's `Open` method takes it.
            _ if entry.dbus_activatable && tile.profile.is_none() && action.is_none() => {
                return Ok(Plan::DBus {
                    desktop_id: entry.id.clone(),
                });
            }
            Some(argv) => argv,
            None => return Err(PlanError::NoExec(entry.name.clone())),
        },
    };
    let mut tokens = base.to_vec();
    if let Some(adapter) = adapter_for(&entry.id) {
        if let Some(profile) = &tile.profile {
            // Identity comes from the main Exec the profile was discovered with (it may carry
            // `--user-data-dir`); an action Exec without that selector inherits it.
            let main = entry.exec.as_deref().unwrap_or(base);
            let main_launcher = launcher(main)?;
            if !revalidate(adapter, &main_launcher, env, main, profile) {
                return Err(PlanError::StaleProfile(profile.label.clone()));
            }
            // The profile key only means something in the main Exec's store.
            if let Some(TileAction::Desktop { id, .. }) = action
                && !same_store(adapter, &main_launcher, main, base)
            {
                return Err(PlanError::ConflictingRoot(id.clone()));
            }
            let mut extra = Vec::new();
            if let Some(root) = user_data_dir(main)
                && user_data_dir(&tokens).is_none()
            {
                extra.push(format!("--user-data-dir={}", root.display()));
            }
            extra.extend(adapter.store.profile_args(profile));
            // All literal data, but `expand` below reads `%` as a field code. The folder never has one (discovery
            // refuses it, see `profiles::store_root`); a profile's name can.
            let extra: Vec<String> = extra.iter().map(|arg| arg.replace('%', "%%")).collect();
            tokens = insert_args(&tokens, &extra, adapter.store.selectors())?;
        }
        if matches!(action, Some(TileAction::Private)) {
            tokens = insert_args(&tokens, &[adapter.private.to_owned()], &[])?;
        }
    }
    let ctx = Context {
        uri,
        icon: entry.icon.as_deref(),
        name: &entry.name,
        desktop_file: Some(&entry.file),
    };
    let mut argv = expand_written(base, &tokens, &ctx)?;
    if entry.terminal {
        argv = wrap_in_terminal(argv, terminal)?;
    }
    Ok(Plan::Exec(LaunchSpec {
        argv,
        cwd: entry.work_dir.clone(),
        token: None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::AppAction;
    use std::path::PathBuf;

    const URI: &str = "https://ex.org/a b?q='1'&r=\"2\"#f%20";

    fn home(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/homes")
            .join(name)
    }

    pub(super) fn entry(
        id: &str,
        name: &str,
        exec: &[&str],
        actions: &[(&str, &[&str])],
    ) -> AppEntry {
        AppEntry {
            id: id.into(),
            name: name.into(),
            icon: Some(id.into()),
            exec: Some(exec.iter().map(|s| (*s).to_owned()).collect()),
            exec_error: None,
            try_exec: None,
            work_dir: None,
            terminal: false,
            dbus_activatable: false,
            no_display: false,
            hidden: false,
            mime_types: vec!["x-scheme-handler/https".into()],
            actions: actions
                .iter()
                .map(|(id, exec)| AppAction {
                    id: (*id).into(),
                    name: format!("{id} label"),
                    exec: exec.iter().map(|s| (*s).to_owned()).collect(),
                })
                .collect(),
            file: PathBuf::from(format!("/usr/share/applications/{id}.desktop")),
        }
    }

    fn with_home<T>(name: &str, f: impl FnOnce(&Env) -> T) -> T {
        let h = home(name);
        let c = h.join(".config");
        f(&Env {
            home: &h,
            config_home: &c,
            view: &crate::index::Native,
        })
    }

    fn tile_for(entry: &AppEntry, env: &Env, label: &str) -> Tile {
        crate::profiles::tiles_for(entry, env)
            .into_iter()
            .find(|t| t.label == label)
            .unwrap()
    }

    fn argv(p: Plan) -> Vec<String> {
        match p {
            Plan::Exec(spec) => spec.argv,
            Plan::DBus { .. } => panic!("expected exec"),
        }
    }

    #[test]
    fn the_icon_code_passes_the_desktop_entrys_own_icon_whatever_the_tile_shows() {
        with_home("a", |env| {
            let mut e = entry("app", "App", &["app", "%i", "%u"], &[]);
            e.icon = Some("/usr/share/icons/app.png".into());
            let mut t = tile_for(&e, env, "App");
            t.icon = Some("/run/host/usr/share/icons/app.png".into());
            assert_eq!(
                argv(plan(&e, &t, None, URI, env, "x").unwrap()),
                ["app", "--icon", "/usr/share/icons/app.png", URI]
            );
        });
    }

    #[test]
    fn chrome_profile_plan() {
        with_home("a", |env| {
            let e = entry(
                "google-chrome",
                "Google Chrome",
                &["/usr/bin/google-chrome-stable", "%U"],
                &[],
            );
            let t = tile_for(&e, env, "travel");
            assert_eq!(
                argv(plan(&e, &t, None, URI, env, "cosmic-term").unwrap()),
                [
                    "/usr/bin/google-chrome-stable",
                    "--profile-directory=Profile 1",
                    URI
                ]
            );
        });
    }

    #[test]
    fn firefox_profile_name_with_space_is_one_argument() {
        with_home("a", |env| {
            let e = entry("firefox", "Firefox", &["firefox", "%u"], &[]);
            let t = tile_for(&e, env, "Work Stuff");
            assert_eq!(
                argv(plan(&e, &t, None, URI, env, "x").unwrap()),
                ["firefox", "-P", "Work Stuff", URI]
            );
        });
    }

    #[test]
    fn private_and_desktop_actions() {
        with_home("a", |env| {
            let e = entry(
                "firefox",
                "Firefox",
                &["firefox", "%u"],
                &[("new-window", &["firefox", "--new-window", "%u"])],
            );
            let t = tile_for(&e, env, "default-release");
            assert_eq!(
                argv(plan(&e, &t, Some(&TileAction::Private), URI, env, "x").unwrap()),
                ["firefox", "-P", "default-release", "--private-window", URI]
            );
            let action = TileAction::Desktop {
                id: "new-window".into(),
                label: "x".into(),
            };
            assert_eq!(
                argv(plan(&e, &t, Some(&action), URI, env, "x").unwrap()),
                ["firefox", "--new-window", "-P", "default-release", URI]
            );
            let missing = TileAction::Desktop {
                id: "nope".into(),
                label: "x".into(),
            };
            assert_eq!(
                plan(&e, &t, Some(&missing), URI, env, "x"),
                Err(PlanError::ActionMissing("nope".into()))
            );
        });
    }

    #[test]
    fn desktop_action_inherits_the_main_execs_user_data_dir() {
        with_home("a", |env| {
            let custom = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/homes/custom-root/chrome-data");
            let flag = format!("--user-data-dir={}", custom.display());
            let e = entry(
                "google-chrome",
                "Chrome",
                &["/usr/bin/google-chrome-stable", &flag, "%U"],
                &[(
                    "new-window",
                    &["/usr/bin/google-chrome-stable", "--new-window", "%U"],
                )],
            );
            let t = tile_for(&e, env, "Alt Two");
            let action = TileAction::Desktop {
                id: "new-window".into(),
                label: "x".into(),
            };
            assert_eq!(
                argv(plan(&e, &t, Some(&action), URI, env, "x").unwrap()),
                [
                    "/usr/bin/google-chrome-stable",
                    "--new-window",
                    &flag,
                    "--profile-directory=Profile 5",
                    URI
                ]
            );
        });
    }

    #[test]
    fn desktop_action_with_a_conflicting_root_is_rejected() {
        with_home("a", |env| {
            let custom = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/homes/custom-root/chrome-data");
            let flag = format!("--user-data-dir={}", custom.display());
            let e = entry(
                "google-chrome",
                "Chrome",
                &["/usr/bin/google-chrome-stable", &flag, "%U"],
                &[(
                    "other-root",
                    &[
                        "/usr/bin/google-chrome-stable",
                        "--user-data-dir=/somewhere/else",
                        "%U",
                    ],
                )],
            );
            let t = tile_for(&e, env, "Alt Two");
            let action = TileAction::Desktop {
                id: "other-root".into(),
                label: "x".into(),
            };
            assert_eq!(
                plan(&e, &t, Some(&action), URI, env, "x"),
                Err(PlanError::ConflictingRoot("other-root".into()))
            );
        });
    }

    #[test]
    fn desktop_action_reaching_a_different_launcher_store_is_rejected() {
        with_home("a", |env| {
            let e = entry(
                "firefox",
                "Firefox",
                &["firefox", "%u"],
                &[(
                    "flatpak-window",
                    &["flatpak", "run", "org.mozilla.firefox", "%u"],
                )],
            );
            let t = tile_for(&e, env, "Work Stuff");
            let action = TileAction::Desktop {
                id: "flatpak-window".into(),
                label: "x".into(),
            };
            assert_eq!(
                plan(&e, &t, Some(&action), URI, env, "x"),
                Err(PlanError::ConflictingRoot("flatpak-window".into()))
            );
        });
    }

    #[test]
    fn profile_names_stay_literal_through_field_code_expansion() {
        with_home("percent", |env| {
            let e = entry(
                "firefox",
                "Firefox",
                &["firefox", "%u"],
                &[("new-window", &["firefox", "--new-window", "%u"])],
            );
            let action = TileAction::Desktop {
                id: "new-window".into(),
                label: "x".into(),
            };
            let (mut got, mut want) = (Vec::new(), Vec::new());
            for name in ["50%", "Work %c", "%f"] {
                let t = tile_for(&e, env, name);
                for (a, rest) in [
                    (None, vec!["-P", name, URI]),
                    (
                        Some(&TileAction::Private),
                        vec!["-P", name, "--private-window", URI],
                    ),
                    (Some(&action), vec!["--new-window", "-P", name, URI]),
                ] {
                    got.push(plan(&e, &t, a, URI, env, "x").map(argv));
                    let mut expected = vec!["firefox".to_owned()];
                    expected.extend(rest.into_iter().map(str::to_owned));
                    want.push(Ok(expected));
                }
            }
            assert_eq!(got, want);
        });
    }

    #[test]
    fn wrapper_plain_tile_launches_with_uri() {
        with_home("a", |env| {
            let e = entry(
                "firefox",
                "Firefox",
                &["env", "MOZ_X=1", "firefox", "%u"],
                &[],
            );
            let t = tile_for(&e, env, "Firefox");
            assert_eq!(
                argv(plan(&e, &t, None, URI, env, "x").unwrap()),
                ["env", "MOZ_X=1", "firefox", URI]
            );
        });
    }

    #[test]
    fn a_hostile_url_stays_one_literal_argument_and_never_reaches_a_shell() {
        const HOSTILE: &str =
            "https://example.org/$(touch subst)`touch tick`;touch semi&&touch and|touch pipe'\"q";
        with_home("a", |env| {
            // A non-shell wrapper: the URL is one argv element, printed back byte for byte.
            let wrapped = entry("wrapped", "Wrapped", &["env", "printf", "%%s", "%u"], &[]);
            let t = tile_for(&wrapped, env, "Wrapped");
            let argv = argv(plan(&wrapped, &t, None, HOSTILE, env, "x").unwrap());
            assert_eq!(argv, ["env", "printf", "%s", HOSTILE]);
            let cwd = tempfile::tempdir().unwrap();
            let out = {
                let _fork = spawn::tests::script_lock();
                std::process::Command::new(&argv[0])
                    .args(&argv[1..])
                    .current_dir(cwd.path())
                    .output()
                    .unwrap()
            };
            assert_eq!(String::from_utf8_lossy(&out.stdout), HOSTILE);
            assert_eq!(
                std::fs::read_dir(cwd.path()).unwrap().count(),
                0,
                "a command ran"
            );

            // Any shell receiving the URL is refused, as the main Exec or as a desktop action.
            let action = TileAction::Desktop {
                id: "shell".into(),
                label: "x".into(),
            };
            let with_action = entry(
                "wrapped",
                "Wrapped",
                &["env", "printf", "%%s", "%u"],
                &[("shell", &["sh", "-c", "printf '%%s' %u"])],
            );
            let mut cases = vec![(with_action, Some(&action))];
            for exec in [
                &["sh", "-c", "printf '%%s' %u"][..],
                &["sh", "-c", r#"printf '%%s' "$1""#, "sh", "%u"],
                &["bash", "--rcfile", "/dev/null", "-lc", "printf '%%s' %u"],
                &["sh", "-c", "%d", "printf '%%s' %u"],
                &[
                    "env",
                    "A=1",
                    "flatpak",
                    "run",
                    "--command=sh",
                    "org.x.App",
                    "-c",
                    "x %u",
                ],
            ] {
                cases.push((entry("wrapped", "Wrapped", exec, &[]), None));
            }
            for (e, a) in &cases {
                assert_eq!(
                    plan(e, &t, *a, HOSTILE, env, "x"),
                    Err(PlanError::Exec(ExecError::ShellReceivesFieldCode)),
                    "{:?}",
                    e.exec
                );
            }
        });
    }

    #[test]
    fn stale_profile_is_an_error() {
        with_home("a", |env| {
            let e = entry("firefox", "Firefox", &["firefox", "%u"], &[]);
            let mut t = tile_for(&e, env, "Work Stuff");
            t.profile.as_mut().unwrap().key = "Vanished".into();
            assert_eq!(
                plan(&e, &t, None, URI, env, "x"),
                Err(PlanError::StaleProfile("Work Stuff".into()))
            );
        });
    }

    #[test]
    fn dbus_only_terminal_and_working_dir() {
        with_home("a", |env| {
            let mut d = entry("org.example.DbusOnly", "Dbus Only", &[], &[]);
            d.exec = None;
            d.dbus_activatable = true;
            let t = tile_for(&d, env, "Dbus Only");
            assert_eq!(
                plan(&d, &t, None, URI, env, "x"),
                Ok(Plan::DBus {
                    desktop_id: "org.example.DbusOnly".into()
                })
            );
            // Its compatibility Exec cannot carry the link; its `Open` method can.
            let mut compat = entry("org.example.Compat", "Compat", &["compat-app"], &[]);
            compat.dbus_activatable = true;
            let t = tile_for(&compat, env, "Compat");
            assert_eq!(
                plan(&compat, &t, None, URI, env, "x"),
                Ok(Plan::DBus {
                    desktop_id: "org.example.Compat".into()
                })
            );
            let mut term = entry("term", "Term", &["lynx", "%u"], &[]);
            term.terminal = true;
            term.work_dir = Some("/tmp".into());
            let t = tile_for(&term, env, "Term");
            assert_eq!(
                plan(&term, &t, None, URI, env, "cosmic-term --x"),
                Ok(Plan::Exec(LaunchSpec {
                    argv: vec![
                        "cosmic-term".into(),
                        "--x".into(),
                        "-e".into(),
                        "lynx".into(),
                        URI.into()
                    ],
                    cwd: Some("/tmp".into()),
                    token: None
                }))
            );
        });
    }

    #[test]
    fn a_profile_named_like_a_shell_is_data_never_a_shell() {
        with_home("shell-names", |env| {
            let e = entry("firefox", "Firefox", &["firefox", "%u"], &[]);
            for name in ["sh", "bash"] {
                let t = tile_for(&e, env, name);
                assert_eq!(
                    argv(plan(&e, &t, None, URI, env, "x").unwrap()),
                    ["firefox", "-P", name, URI]
                );
            }
        });
    }
}
