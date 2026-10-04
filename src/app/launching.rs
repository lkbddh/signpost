//! Launching the app a picker chose: the attempt each open picker keeps, the plan run here or on the host, and
//! what comes back from it.

use cosmic::app::Task;
use cosmic::cctk::sctk::shell::wlr_layer::KeyboardInteractivity;
use cosmic::iced::platform_specific::shell::commands::layer_surface::set_keyboard_interactivity;
use cosmic::iced::window;

use super::{App, Message, snap_to_focus, view_of};
use crate::fl;
use crate::host::Host;
use crate::launch::{Plan, plan};
use crate::picker::{LaunchFailure, Picker, Settled, Target};
use crate::profiles::{Env, TileAction};

#[derive(Clone)]
pub(super) struct Pending {
    pub(super) attempt: u64,
    pub(super) index: usize,
    pub(super) action: Option<TileAction>,
    pub(super) keep_open: bool,
    /// The token arrived (or timed out) and the launch is running.
    pub(super) launching: bool,
    /// The popup of the menu that chose this launch: the token is asked from it, since it has the input that
    /// chose it, and it goes once the token is in.
    pub(super) popup: Option<window::Id>,
}

/// An open picker: its model, and what the app keeps beside it while it is open.
pub(super) struct PickerState {
    pub(super) picker: Picker,
    pub(super) apps: Vec<crate::index::AppEntry>,
    pub(super) pending: Option<Pending>,
    /// The launch being started: a D-Bus call, or a Flatpak's check and spawn on the host. It is aborted when
    /// this state is dropped, so a picker that closed keeps no call waiting on an app that may never answer and
    /// spawns nothing it had not spawned yet. A request an app already received is that app's to finish.
    pub(super) starting: Option<cosmic::iced::task::Handle>,
    /// Finger and origin of an in-progress touch, for slop-based long-press cancellation.
    pub(super) touch: Option<(cosmic::iced::touch::Finger, (f32, f32))>,
}

impl PickerState {
    pub(super) fn new(picker: Picker, apps: Vec<crate::index::AppEntry>) -> Self {
        Self {
            picker,
            apps,
            pending: None,
            starting: None,
            touch: None,
        }
    }

    /// Admits a launch the picker emitted: starts `attempt` unless one is in flight. Only an admitted
    /// keep-open launch makes the picker keep-open, set before the target starts (it may take focus
    /// before its launch result arrives). An admitted attempt also dismisses the previous failure.
    pub(super) fn admit(&mut self, attempt: Pending) -> bool {
        let keep_open = attempt.keep_open;
        if !self.begin(attempt) {
            return false;
        }
        self.picker.keep_open |= keep_open;
        self.picker.clear_failure();
        true
    }

    /// Starts `attempt` unless another launch attempt is still pending (one at a time per picker).
    pub(super) fn begin(&mut self, attempt: Pending) -> bool {
        if self.pending.is_some() {
            return false;
        }
        self.pending = Some(attempt);
        true
    }

    /// `attempt` still waits for its activation token.
    pub(super) fn awaiting_token(&self, attempt: u64) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|p| p.attempt == attempt && !p.launching)
    }

    /// The launch awaiting its token holds `popup`.
    pub(super) fn holds(&self, popup: window::Id) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|pending| pending.popup == Some(popup))
    }

    /// The popup the launch holds, handed over to be destroyed: it is held no more.
    pub(super) fn take_held_popup(&mut self) -> Option<window::Id> {
        self.pending
            .as_mut()
            .and_then(|pending| pending.popup.take())
    }

    /// The token of `attempt` arrived (or timed out): its launch starts, once. It stays pending, so
    /// further selections are ignored until the launch completes.
    pub(super) fn start(&mut self, attempt: u64) -> Option<Pending> {
        let p = self
            .pending
            .as_mut()
            .filter(|p| p.attempt == attempt && !p.launching)?;
        p.launching = true;
        Some(p.clone())
    }

    /// The launch of `attempt` completed; a stale or reordered result finds nothing.
    pub(super) fn finish(&mut self, attempt: u64) -> Option<Pending> {
        let done = self
            .pending
            .take_if(|p| p.attempt == attempt && p.launching)?;
        self.starting = None;
        Some(done)
    }
}

impl App {
    /// The launch of `attempt` starts, with `token`; the popup of the menu that chose it has done its part.
    pub(super) fn launch(
        &mut self,
        id: window::Id,
        attempt: u64,
        token: Option<String>,
    ) -> Task<Message> {
        let release = self.release_popup(id, attempt);
        Task::batch([release, self.run_launch(id, attempt, token)])
    }

    fn run_launch(&mut self, id: window::Id, attempt: u64, token: Option<String>) -> Task<Message> {
        let Some(state) = self.pickers.get_mut(&id) else {
            return Task::none();
        };
        let Some(pending) = state.start(attempt) else {
            return Task::none();
        };
        let target = state.picker.tiles.get(pending.index).and_then(|tile| {
            let entry = state.apps.iter().find(|e| e.id == tile.app_id)?.clone();
            Some((state.picker.uri.clone(), tile.clone(), entry))
        });
        let Some((uri, tile, entry)) = target else {
            state.finish(attempt);
            return Task::none();
        };
        let env = Env {
            home: &self.paths.home,
            config_home: &self.paths.config_home,
            view: view_of(&self.host),
        };
        let terminal = crate::launch::spawn::configured_terminal();
        let fail = |message: String| launched_now(id, attempt, Some(message));
        match plan(
            &entry,
            &tile,
            pending.action.as_ref(),
            &uri,
            &env,
            &terminal,
        ) {
            Err(e) => fail(e.to_string()),
            Ok(Plan::DBus { desktop_id }) => {
                let conn = self.conn.clone();
                let (launch, handle) = Task::perform(
                    async move {
                        crate::launch::dbus::open_via_dbus(
                            &conn,
                            &desktop_id,
                            &uri,
                            token.as_deref(),
                        )
                        .await
                        .err()
                        .map(|e| e.to_string())
                    },
                    move |error| cosmic::Action::App(Message::Launched { id, attempt, error }),
                )
                .abortable();
                state.starting = Some(handle.abort_on_drop());
                launch
            }
            Ok(Plan::Exec(mut spec)) => {
                spec.token = token;
                let Host::Flatpak(host) = &self.host else {
                    return spawn_exec(
                        id,
                        attempt,
                        uri,
                        Target::from(&tile),
                        &spec,
                        &entry,
                        &self.conn,
                    );
                };
                let (launch, handle) =
                    spawn_on_host(id, attempt, uri, Target::from(&tile), spec, &entry, host);
                state.starting = Some(handle.abort_on_drop());
                launch
            }
        }
    }

    pub(super) fn launched(
        &mut self,
        id: window::Id,
        attempt: u64,
        error: Option<String>,
    ) -> Task<Message> {
        // Only the attempt in flight settles the picker; anything else is stale.
        let Some(state) = self.pickers.get_mut(&id) else {
            return Task::none();
        };
        let Some(pending) = state.finish(attempt) else {
            return Task::none();
        };
        match state.picker.settle(pending.index, pending.keep_open, error) {
            Settled::Failed => snap_to_focus(&state.picker),
            Settled::Close => self.close_picker(id),
            Settled::KeepOpen { relax_focus: true } => {
                set_keyboard_interactivity(id, KeyboardInteractivity::OnDemand)
            }
            Settled::KeepOpen { relax_focus: false } => Task::none(),
        }
    }
}

/// The `Launched` result of a launch that finished (or failed) synchronously.
fn launched_now(id: window::Id, attempt: u64, error: Option<String>) -> Task<Message> {
    Task::done(cosmic::Action::App(Message::Launched {
        id,
        attempt,
        error,
    }))
}

/// Pre-check and spawn an Exec plan; a non-zero exit within the early-failure window comes back as
/// `EarlyFailure` for the source picker.
pub(super) fn spawn_exec(
    id: window::Id,
    attempt: u64,
    uri: String,
    failed: Target,
    spec: &crate::launch::spawn::LaunchSpec,
    entry: &crate::index::AppEntry,
    conn: &zbus::Connection,
) -> Task<Message> {
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    if let Err(e) = crate::launch::spawn::precheck(spec, entry.try_exec.as_deref(), &path_var) {
        return launched_now(id, attempt, Some(e.to_string()));
    }
    match crate::launch::spawn::spawn_detached(spec, crate::launch::spawn::EARLY_FAILURE_WINDOW) {
        Err(e) => launched_now(id, attempt, Some(e.to_string())),
        Ok((pid, gate)) => {
            tracing::info!(pid, app = %entry.id, "launched");
            let (conn, entry) = (conn.clone(), entry.clone());
            Task::batch([
                launched_now(id, attempt, None),
                Task::perform(
                    async move {
                        crate::launch::scope::enter_own_scope(&conn, &entry, pid).await;
                        gate.open().await
                    },
                    move |failure| exec_failure(id, attempt, uri, failed, failure),
                ),
            ])
        }
    }
}

/// [`spawn_exec`] from a Flatpak: the plan is checked and run on the host, in a scope of its own there when the
/// host makes them. Until it is spawned, the launch can be aborted through the handle; from then on, its gate and
/// reaper own it, as they do here.
pub(super) fn spawn_on_host(
    id: window::Id,
    attempt: u64,
    uri: String,
    failed: Target,
    spec: crate::launch::spawn::LaunchSpec,
    entry: &crate::index::AppEntry,
    host: &crate::host::HostEnv,
) -> (Task<Message>, cosmic::iced::task::Handle) {
    let scopes = host.scopes;
    let unit = crate::launch::host::scope_unit(&entry.id, crate::host::unit_suffix());
    // Without a folder of its own, the app starts in the host's home, as a launcher on the host would start it: the
    // folder Signpost runs in is the sandbox's, which the host may not have.
    let launched = crate::launch::spawn::LaunchSpec {
        cwd: spec.cwd.clone().or_else(|| Some(host.home.clone())),
        ..spec.clone()
    };
    let wrapped = crate::launch::host::on_host(&launched, &unit, &entry.name, scopes);
    // The arguments can carry the link, `env` assignments and other private details, so only their count is logged.
    tracing::debug!(app = %entry.id, args = spec.argv.len(), unit, scopes, token = spec.token.is_some(), "launching on the host");
    let try_exec = entry.try_exec.clone();
    let (spawned, handle) = cosmic::iced::Task::perform(
        async move {
            // Checked where it will start, as a relative PATH entry is resolved from there.
            crate::launch::host::precheck(
                &launched,
                try_exec.as_deref(),
                crate::host::on_host,
                crate::host::ASK_TIMEOUT,
            )
            .await
            .map_err(|e| e.to_string())?;
            let (pid, gate) = crate::launch::spawn::spawn_detached(
                &wrapped,
                crate::launch::spawn::EARLY_FAILURE_WINDOW,
            )
            .map_err(|e| e.to_string())?;
            // The gate ran `flatpak-spawn`; the failure is the app's, whose status it passes on.
            let program = spec.argv.first().cloned().unwrap_or_default();
            Ok((pid, gate, program))
        },
        std::convert::identity,
    )
    .abortable();
    let app = entry.id.clone();
    let launch = spawned.then(move |spawned| match spawned {
        Err(error) => launched_now(id, attempt, Some(error)),
        Ok((pid, gate, program)) => {
            tracing::info!(pid, app = %app, "launched on the host");
            let (uri, failed) = (uri.clone(), failed.clone());
            Task::batch([
                launched_now(id, attempt, None),
                Task::perform(
                    async move {
                        let failure = gate.open().await;
                        failure.map(|failure| crate::launch::spawn::EarlyFailure {
                            program,
                            ..failure
                        })
                    },
                    move |failure| exec_failure(id, attempt, uri, failed, failure),
                ),
            ])
        }
    });
    (launch, handle)
}

/// What an exec launch of `attempt` from picker `id` sends once its early-failure window has passed.
fn exec_failure(
    id: window::Id,
    attempt: u64,
    uri: String,
    failed: Target,
    failure: Option<crate::launch::spawn::EarlyFailure>,
) -> cosmic::Action<Message> {
    match failure {
        Some(f) => cosmic::Action::App(Message::EarlyFailure {
            source: id,
            attempt,
            failure: LaunchFailure {
                uri,
                failed,
                message: fl!(
                    "early-failure",
                    program = f.program,
                    code = f
                        .code
                        .map_or_else(|| "signal".to_owned(), |c| c.to_string())
                ),
            },
        }),
        None => cosmic::Action::None,
    }
}
