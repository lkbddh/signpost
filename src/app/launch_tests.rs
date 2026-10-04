//! Launches as the app runs them, against fake peers in place of the session bus.
//!
//! Building the app reads libcosmic's configuration and the app index, so the tests that build it run
//! again in a child process whose home is a scratch directory.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cosmic::iced::runtime::Action as RuntimeAction;
use futures_util::StreamExt;
use tokio::sync::{Notify, mpsc};
use zbus::zvariant::Value;

use super::launching::spawn_exec;
use super::world::World;
use super::*;
use crate::index::AppEntry;
use crate::launch::spawn::{LaunchSpec, tests::script_lock};
use crate::picker::Target;
use crate::profiles::Tile;
use crate::test_support::headless;
use crate::test_support::peer::{self, AnsweringApp, FakeManager, SYSTEMD_PATH, SilentApp};

const SCRATCH_HOME: &str = "SIGNPOST_APP_LAUNCH_SCRATCH_HOME";
const LINK: &str = "https://example.org/";
const DBUS_APP: &str = "org.example.App";
const DBUS_APP_PATH: &str = "/org/example/App";
/// How long a launch that should end is waited for before the test calls it hung. Only a hang guard: a launch
/// runs through its scope attempt and its early-failure window, and a loaded machine stretches both.
const SETTLE: Duration = Duration::from_secs(30);
/// How long a program that must not have started yet is given to start anyway.
const HELD: Duration = Duration::from_millis(300);

/// Runs test `name` again with its home in a scratch directory, and says whether this is that run.
fn in_scratch_home(name: &str) -> bool {
    headless::in_scratch_home(SCRATCH_HOME, &format!("app::launch_tests::{name}"))
}

fn tile(app: &str) -> Tile {
    Tile {
        app_id: app.into(),
        app_name: app.into(),
        label: app.into(),
        icon: None,
        profile: None,
        flatpak: false,
        actions: Vec::new(),
    }
}

fn dbus_app(id: &str) -> AppEntry {
    AppEntry {
        id: id.into(),
        name: id.into(),
        icon: None,
        exec: None,
        exec_error: None,
        try_exec: None,
        work_dir: None,
        terminal: false,
        dbus_activatable: true,
        no_display: false,
        hidden: false,
        mime_types: vec!["x-scheme-handler/https".into()],
        actions: Vec::new(),
        file: PathBuf::from(format!("/usr/share/applications/{id}.desktop")),
    }
}

/// A picker for [`DBUS_APP`] with its launch attempt 1 waiting for the token, and the id of its surface.
fn picker_waiting_to_launch(world: &mut World) -> window::Id {
    picker_waiting_to_launch_app(world, dbus_app(DBUS_APP))
}

/// A picker for `app` with its launch attempt 1 waiting for the token, and the id of its surface.
fn picker_waiting_to_launch_app(world: &mut World, app: AppEntry) -> window::Id {
    let id = window::Id::unique();
    let picker = Picker::new(LINK.into(), vec![tile(&app.id)]);
    world.app.pickers.insert(
        id,
        PickerState {
            pending: Some(Pending {
                attempt: 1,
                index: 0,
                action: None,
                keep_open: false,
                launching: false,
                popup: None,
            }),
            ..PickerState::new(picker, vec![app])
        },
    );
    id
}

/// What the launch task of `id`'s attempt 1 sends, collected on the world's runtime.
fn running_launch(world: &mut World, id: window::Id) -> tokio::task::JoinHandle<Vec<Message>> {
    let launch = world.app.launch(id, 1, None);
    let stream = cosmic::iced::runtime::task::into_stream(launch).expect("a launch is a task");
    world.runtime.spawn(
        stream
            .filter_map(|action| async move {
                let RuntimeAction::Output(cosmic::Action::App(message)) = action else {
                    return None;
                };
                Some(message)
            })
            .collect(),
    )
}

#[test]
fn closing_a_picker_aborts_the_dbus_launch_it_waits_on() {
    if !in_scratch_home("closing_a_picker_aborts_the_dbus_launch_it_waits_on") {
        return;
    }
    let (entered, mut reached) = mpsc::unbounded_channel();
    let mut world = World::serving(|peer| peer.serve_at(DBUS_APP_PATH, SilentApp(entered)));
    let id = picker_waiting_to_launch(&mut world);
    let outcome = running_launch(&mut world, id);
    world
        .runtime
        .block_on(reached.recv())
        .expect("the app received the call");

    drop(world.app.close_picker(id));

    let sent = world
        .runtime
        .block_on(async { tokio::time::timeout(SETTLE, outcome).await })
        .expect("the launch ended with its picker")
        .expect("the task ran");
    assert!(sent.is_empty(), "no Launched arrives for a closed picker");
}

#[test]
fn an_answered_dbus_launch_still_reports_to_its_open_picker() {
    if !in_scratch_home("an_answered_dbus_launch_still_reports_to_its_open_picker") {
        return;
    }
    let mut world = World::serving(|peer| peer.serve_at(DBUS_APP_PATH, AnsweringApp));
    let id = picker_waiting_to_launch(&mut world);
    let outcome = running_launch(&mut world, id);

    let sent = world
        .runtime
        .block_on(async { tokio::time::timeout(SETTLE, outcome).await })
        .expect("the launch ended")
        .expect("the task ran");

    let [Message::Launched { error: None, .. }] = sent.as_slice() else {
        panic!("one successful Launched: {sent:?}");
    };
}

const EXEC_APP: &str = "org.example.Exec";

fn exec_app(argv: &[&Path]) -> AppEntry {
    AppEntry {
        dbus_activatable: false,
        exec: Some(
            argv.iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect(),
        ),
        name: "Example".into(),
        ..dbus_app(EXEC_APP)
    }
}

/// A program that runs `body`.
fn program(dir: &Path, body: &str) -> LaunchSpec {
    let program = dir.join("program");
    let _write = script_lock();
    std::fs::write(&program, format!("#!/bin/sh\n{body}\n")).expect("a script");
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).expect("executable");
    LaunchSpec {
        argv: vec![program.to_string_lossy().into_owned()],
        cwd: None,
        token: None,
    }
}

/// A program that writes its own pid to `pid_file` and exits.
fn pid_writer(dir: &Path, pid_file: &Path) -> LaunchSpec {
    program(dir, &format!("echo $$ > {}", pid_file.display()))
}

/// Runs `launch` (a launch task) to its end, and gives what it sent the app.
async fn sent_by(launch: Task<Message>) -> Vec<Message> {
    let stream = cosmic::iced::runtime::task::into_stream(launch).expect("a launch is a task");
    stream
        .filter_map(|action| async move {
            let RuntimeAction::Output(cosmic::Action::App(message)) = action else {
                return None;
            };
            Some(message)
        })
        .collect()
        .await
}

fn target() -> Target {
    Target {
        app_id: EXEC_APP.into(),
        profile_key: None,
        label: "Example".into(),
    }
}

/// Launches a pid-writing program as the app does, against a manager that answers as `manager` builds it,
/// and gives what the launch sent, the pid the program wrote, and the units the manager was asked for.
async fn launched_against(
    manager: impl FnOnce(mpsc::UnboundedSender<peer::TransientUnit>) -> FakeManager,
) -> (
    Vec<Message>,
    u32,
    mpsc::UnboundedReceiver<peer::TransientUnit>,
) {
    let (asked, units) = mpsc::unbounded_channel();
    let (conn, _manager) =
        peer::connect(|systemd| systemd.serve_at(SYSTEMD_PATH, manager(asked))).await;
    let dir = tempfile::tempdir().expect("a scratch directory");
    let pid_file = dir.path().join("pid");
    let spec = pid_writer(dir.path(), &pid_file);
    let entry = exec_app(&[]);
    let launch = {
        let _fork = script_lock();
        spawn_exec(
            window::Id::unique(),
            1,
            LINK.into(),
            target(),
            &spec,
            &entry,
            &conn,
        )
    };
    let sent = sent_by(launch).await;
    (sent, written_pid(&pid_file).await, units)
}

/// The pid the program wrote to `file`. A launch can end before a program that waited at the gate has run.
async fn written_pid(file: &Path) -> u32 {
    for _ in 0..100 {
        if let Some(pid) = std::fs::read_to_string(file)
            .ok()
            .and_then(|text| text.trim().parse().ok())
        {
            return pid;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the program never wrote its pid to {}", file.display());
}

fn pids_of(unit: &peer::TransientUnit) -> Option<&Value<'_>> {
    unit.properties
        .iter()
        .find(|(name, _)| name == "PIDs")
        .map(|(_, value)| &**value)
}

fn succeeded(sent: &[Message]) -> bool {
    matches!(
        sent.iter().find(|m| matches!(m, Message::Launched { .. })),
        Some(Message::Launched { error: None, .. })
    )
}

#[tokio::test]
async fn an_app_started_by_the_picker_runs_in_a_scope_of_its_own() {
    let (sent, pid, mut units) = launched_against(FakeManager::starting).await;

    assert!(succeeded(&sent), "{sent:?}");
    let unit = units.try_recv().expect("the manager was asked");
    assert_eq!(unit.name, format!("app-signpost-{EXEC_APP}-{pid}.scope"));
    assert_eq!(unit.mode, "fail");
    assert_eq!(
        pids_of(&unit),
        Some(&Value::from(vec![pid])),
        "the child's own pid"
    );
    assert!(units.try_recv().is_err(), "in one call");
}

#[tokio::test]
async fn a_manager_that_refuses_still_reports_the_launch_as_started() {
    let (sent, _, mut units) =
        launched_against(|asked| FakeManager::refusing(asked, "no such luck")).await;

    assert!(units.try_recv().is_ok(), "the manager was asked");
    assert!(succeeded(&sent), "{sent:?}");
}

#[tokio::test]
async fn a_manager_that_never_answers_does_not_keep_the_app_from_running() {
    let launch = launched_against(FakeManager::silent);
    let (sent, _, mut units) = tokio::time::timeout(SETTLE, launch)
        .await
        .expect("the launch never ended");

    assert!(units.try_recv().is_ok(), "the manager was asked");
    assert!(succeeded(&sent), "{sent:?}");
}

#[tokio::test]
async fn an_app_does_not_run_until_its_scope_attempt_has_ended() {
    let release = Arc::new(Notify::new());
    let (asked, mut units) = mpsc::unbounded_channel();
    let (conn, _manager) = peer::connect(|systemd| {
        systemd.serve_at(
            SYSTEMD_PATH,
            FakeManager::holding(asked, Arc::clone(&release)),
        )
    })
    .await;
    let dir = tempfile::tempdir().expect("a scratch directory");
    let pid_file = dir.path().join("pid");
    let spec = pid_writer(dir.path(), &pid_file);
    let launch = {
        let _fork = script_lock();
        spawn_exec(
            window::Id::unique(),
            1,
            LINK.into(),
            target(),
            &spec,
            &exec_app(&[]),
            &conn,
        )
    };
    let running = tokio::spawn(sent_by(launch));
    let unit = units.recv().await.expect("the manager was asked");

    tokio::time::sleep(HELD).await;
    assert!(
        !pid_file.exists(),
        "the program ran before it was in its scope"
    );

    release.notify_one();
    let sent = tokio::time::timeout(SETTLE, running)
        .await
        .expect("the launch ended once the manager answered")
        .expect("the task ran");
    assert!(succeeded(&sent), "{sent:?}");
    let pid = written_pid(&pid_file).await;
    assert_eq!(
        pids_of(&unit),
        Some(&Value::from(vec![pid])),
        "the scope was asked about the pid that became the program"
    );
}

#[tokio::test]
async fn an_app_that_fails_soon_after_it_starts_is_an_early_failure_however_long_its_scope_took() {
    let (asked, _units) = mpsc::unbounded_channel();
    let (conn, _manager) =
        peer::connect(|systemd| systemd.serve_at(SYSTEMD_PATH, FakeManager::silent(asked))).await;
    let dir = tempfile::tempdir().expect("a scratch directory");
    // The manager is waited on for `SCOPE_TIMEOUT`, so the app fails after the window would have ended had it
    // begun when the program was spawned.
    let spec = program(dir.path(), "sleep 1.3; exit 7");
    let launch = {
        let _fork = script_lock();
        spawn_exec(
            window::Id::unique(),
            1,
            LINK.into(),
            target(),
            &spec,
            &exec_app(&[]),
            &conn,
        )
    };

    let sent = sent_by(launch).await;

    assert!(
        sent.iter()
            .any(|message| matches!(message, Message::EarlyFailure { attempt: 1, .. })),
        "{sent:?}"
    );
}

const FLATPAK_SCRATCH_HOME: &str = "SIGNPOST_APP_FLATPAK_LAUNCH_SCRATCH_HOME";

/// Like [`in_scratch_home`], for the Flatpak launches: `<home>/bin` is first on the child's `PATH`.
fn flatpak_in_scratch_home(name: &str) -> bool {
    headless::in_scratch_home(FLATPAK_SCRATCH_HOME, &format!("app::launch_tests::{name}"))
}

/// The scratch home of this child run.
fn scratch() -> PathBuf {
    std::env::var_os(FLATPAK_SCRATCH_HOME)
        .map(PathBuf::from)
        .expect("a scratch home")
}

/// Stand-ins for the host's `flatpak-spawn` and `systemd-run` in `<home>/bin`. Each logs its arguments to
/// `<home>/calls`, then runs the command after its own options, here, as the host would: `flatpak-spawn` applies
/// `--directory`, `--env` and `--unset-env`, starting from an environment with a stale activation token. `on_check`
/// runs first when the command is `sh`, the host check.
fn stand_ins(home: &Path, on_check: &str) {
    let bin = home.join("bin");
    std::fs::create_dir_all(&bin).expect("the bin folder");
    let log = r#"printf '%s\036' "$@" >> "$HOME/calls"; printf '\035' >> "$HOME/calls""#;
    let _write = script_lock();
    for (name, body) in [
        (
            "flatpak-spawn",
            format!(
                "{log}\nexport XDG_ACTIVATION_TOKEN=stale DESKTOP_STARTUP_ID=stale\n\
                 while [ $# -gt 0 ]; do case $1 in\n\
                 --) shift; break ;;\n\
                 --directory=*) cd \"${{1#--directory=}}\" || exit 1 ;;\n\
                 --env=*) export \"${{1#--env=}}\" ;;\n\
                 --unset-env=*) unset \"${{1#--unset-env=}}\" ;;\n\
                 --*) ;;\n\
                 *) break ;;\n\
                 esac; shift; done\n\
                 if [ \"$1\" = sh ]; then {on_check}; fi\nexec \"$@\""
            ),
        ),
        (
            "systemd-run",
            format!("{log}\nwhile [ \"$1\" != -- ]; do shift; done; shift\nexec \"$@\""),
        ),
    ] {
        let file = bin.join(name);
        std::fs::write(&file, format!("#!/bin/sh\n{body}\n")).expect("a stand-in");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755))
            .expect("executable");
    }
}

/// The arguments of each call the stand-ins logged, in order.
fn calls(home: &Path) -> Vec<Vec<String>> {
    std::fs::read_to_string(home.join("calls"))
        .unwrap_or_default()
        .split_terminator('\x1d')
        .map(|call| call.split_terminator('\x1e').map(str::to_owned).collect())
        .collect()
}

/// A world in a Flatpak whose host is this machine, as the stand-ins show it, making scopes when `scopes`.
fn flatpak_world(home: &Path, scopes: bool) -> World {
    let mut env =
        crate::host::HostEnv::parse(format!("HOME={}\0", home.display()).as_bytes(), home);
    env.scopes = scopes;
    World::on(Host::Flatpak(env), Ok)
}

/// [`EXEC_APP`] running `body` with the link, as its desktop entry says.
fn host_app(home: &Path, body: &str) -> (AppEntry, String) {
    let program = program(home, body).argv.remove(0);
    let entry = AppEntry {
        exec: Some(vec![program.clone(), "%u".into()]),
        try_exec: Some("sh".into()),
        ..exec_app(&[])
    };
    (entry, program)
}

/// Waits for `file` to appear, running `world`'s tasks meanwhile.
fn appeared(world: &World, file: &Path) -> bool {
    world.runtime.block_on(async {
        for _ in 0..400 {
            if file.exists() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        false
    })
}

/// The messages a launch from `id` sent, once it has ended.
fn ended(world: &mut World, id: window::Id) -> Vec<Message> {
    let outcome = running_launch(world, id);
    world
        .runtime
        .block_on(async { tokio::time::timeout(SETTLE, outcome).await })
        .expect("the launch ended")
        .expect("the task ran")
}

#[test]
fn a_flatpak_launch_is_checked_on_the_host_then_runs_there_in_a_scope() {
    if !flatpak_in_scratch_home(
        "a_flatpak_launch_is_checked_on_the_host_then_runs_there_in_a_scope",
    ) {
        return;
    }
    let home = scratch();
    stand_ins(&home, ":");
    let (entry, program) = host_app(&home, r#"echo "$1" > "$HOME/ran""#);
    let mut world = flatpak_world(&home, true);
    let id = picker_waiting_to_launch_app(&mut world, entry);

    let sent = ended(&mut world, id);

    let [Message::Launched { error: None, .. }] = sent.as_slice() else {
        panic!("one successful Launched: {sent:?}");
    };
    assert!(appeared(&world, &home.join("ran")), "the app ran");
    assert_eq!(
        std::fs::read_to_string(home.join("ran")).expect("its record"),
        format!("{LINK}\n")
    );
    let calls = calls(&home);
    let [check, launch, scope] = calls.as_slice() else {
        panic!("a check, a launch and its scope: {calls:?}");
    };
    assert_eq!(
        check[..5],
        ["--host", "--watch-bus", "--directory=/", "sh", "-c"]
    );
    // Checked in the folder the app starts in: without one of its own, the host's home.
    assert_eq!(
        check[6..],
        ["sh", &program, "+sh", &format!("+{}", home.display())]
    );
    let unit = launch
        .get(11)
        .and_then(|unit| unit.strip_prefix("--unit=app-signpost-org.example.Exec-"))
        .and_then(|unit| unit.strip_suffix(".scope"))
        .unwrap_or_else(|| panic!("a unit of the app's own: {launch:?}"));
    assert!(
        unit.len() == 8 && unit.chars().all(|c| c.is_ascii_hexdigit()),
        "{unit}"
    );
    assert_eq!(
        launch,
        &[
            "--host",
            &format!("--directory={}", home.display()),
            "--unset-env=XDG_ACTIVATION_TOKEN",
            "--unset-env=DESKTOP_STARTUP_ID",
            "--",
            "systemd-run",
            "--user",
            "--scope",
            "--quiet",
            "--collect",
            "--expand-environment=no",
            &launch[11],
            "--description=Example",
            "--",
            &program,
            LINK,
        ]
    );
    assert_eq!(scope[..], launch[6..]);
}

#[test]
fn a_flatpak_app_that_fails_at_once_is_reported_by_its_own_name() {
    if !flatpak_in_scratch_home("a_flatpak_app_that_fails_at_once_is_reported_by_its_own_name") {
        return;
    }
    let home = scratch();
    stand_ins(&home, ":");
    let (entry, program) = host_app(&home, r#": > "$HOME/ran"; sleep 0.5; exit 7"#);
    let mut world = flatpak_world(&home, false);
    let id = picker_waiting_to_launch_app(&mut world, entry);
    let outcome = running_launch(&mut world, id);
    assert!(appeared(&world, &home.join("ran")), "the app ran");

    // Once the app runs, the launch is its gate's: the picker letting go of it changes nothing.
    world.app.pickers.get_mut(&id).expect("the picker").starting = None;
    let sent = world
        .runtime
        .block_on(async { tokio::time::timeout(SETTLE, outcome).await })
        .expect("the launch ended")
        .expect("the task ran");

    let failure = sent
        .iter()
        .find_map(|message| match message {
            Message::EarlyFailure { failure, .. } => Some(failure),
            _ => None,
        })
        .unwrap_or_else(|| panic!("an early failure: {sent:?}"));
    assert_eq!(
        failure.message,
        fl!("early-failure", program = program, code = "7")
    );
    assert_eq!(calls(&home).len(), 2, "a check and a launch, with no scope");
}

#[test]
fn a_flatpak_launch_runs_in_its_entrys_folder_on_the_host() {
    if !flatpak_in_scratch_home("a_flatpak_launch_runs_in_its_entrys_folder_on_the_host") {
        return;
    }
    let home = scratch();
    stand_ins(&home, ":");
    let folder = home.join("work folder");
    std::fs::create_dir_all(&folder).expect("the entry's folder");
    let (mut entry, _) = host_app(&home, r#"pwd > "$HOME/ran""#);
    entry.work_dir = Some(folder.clone());
    let mut world = flatpak_world(&home, false);
    let id = picker_waiting_to_launch_app(&mut world, entry);

    let sent = ended(&mut world, id);

    let [Message::Launched { error: None, .. }] = sent.as_slice() else {
        panic!("one successful Launched: {sent:?}");
    };
    assert!(appeared(&world, &home.join("ran")), "the app ran");
    assert_eq!(
        std::fs::read_to_string(home.join("ran")).expect("its folder"),
        format!("{}\n", folder.display())
    );
    let calls = calls(&home);
    assert_eq!(
        calls[0].last().map(String::as_str),
        Some(format!("+{}", folder.display()).as_str()),
        "the host checks the folder"
    );
}

#[test]
fn a_flatpak_launch_without_a_folder_runs_in_the_hosts_home() {
    if !flatpak_in_scratch_home("a_flatpak_launch_without_a_folder_runs_in_the_hosts_home") {
        return;
    }
    let home = scratch();
    stand_ins(&home, ":");
    let (entry, _) = host_app(&home, r#"pwd > "$HOME/ran""#);
    let mut world = flatpak_world(&home, false);
    let id = picker_waiting_to_launch_app(&mut world, entry);

    ended(&mut world, id);

    assert!(appeared(&world, &home.join("ran")), "the app ran");
    assert_eq!(
        std::fs::read_to_string(home.join("ran")).expect("its folder"),
        format!("{}\n", home.display())
    );
}

#[test]
fn a_flatpak_launch_hands_the_app_its_token_and_never_a_stale_one() {
    if !flatpak_in_scratch_home("a_flatpak_launch_hands_the_app_its_token_and_never_a_stale_one") {
        return;
    }
    let home = scratch();
    stand_ins(&home, ":");
    let body = r#"echo "$XDG_ACTIVATION_TOKEN|$DESKTOP_STARTUP_ID" > "$HOME/ran.$$"; mv "$HOME/ran.$$" "$HOME/ran""#;
    for (token, seen) in [(Some("fresh"), "fresh|fresh"), (None, "|")] {
        let _ = std::fs::remove_file(home.join("ran"));
        let (entry, _) = host_app(&home, body);
        let mut world = flatpak_world(&home, false);
        let id = picker_waiting_to_launch_app(&mut world, entry);
        let launch = world.app.launch(id, 1, token.map(str::to_owned));
        world.runtime.block_on(sent_by(launch));
        assert!(appeared(&world, &home.join("ran")), "the app ran");
        assert_eq!(
            std::fs::read_to_string(home.join("ran")).expect("its tokens"),
            format!("{seen}\n"),
            "{token:?}"
        );
    }
}

#[test]
fn closing_a_picker_during_its_host_check_launches_nothing() {
    if !flatpak_in_scratch_home("closing_a_picker_during_its_host_check_launches_nothing") {
        return;
    }
    let home = scratch();
    stand_ins(&home, r#": > "$HOME/checking"; sleep 1"#);
    let (entry, _) = host_app(&home, r#": > "$HOME/ran""#);
    let mut world = flatpak_world(&home, false);
    let id = picker_waiting_to_launch_app(&mut world, entry);
    let outcome = running_launch(&mut world, id);
    assert!(
        appeared(&world, &home.join("checking")),
        "the host check began"
    );

    drop(world.app.close_picker(id));

    let sent = world
        .runtime
        .block_on(async { tokio::time::timeout(SETTLE, outcome).await })
        .expect("the launch ended with its picker")
        .expect("the task ran");
    assert!(
        sent.is_empty(),
        "no Launched arrives for a closed picker: {sent:?}"
    );
    // Past the check's end, nothing more was asked of the host and nothing ran.
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(calls(&home).len(), 1, "{:?}", calls(&home));
    assert!(!home.join("ran").exists(), "the app ran");
}
