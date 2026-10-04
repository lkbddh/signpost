//! The settings window: which page is open, what the main page shows and the operations it runs.

use std::cell::Cell;
use std::path::{Path, PathBuf};

use cosmic::app::Task;
use cosmic::iced::keyboard::{self, key::Named};
use cosmic::iced::{Color, Size, window};
use cosmic::widget::{self, ToastId, Toasts, svg};

use super::drawer::{Drawer, Swatch};
use super::{Action, AppRow, Identity, Model, Op, OpError, Toast, inventory, model};
use crate::bus::APP_ID;
use crate::colors::{ColorStore, Overrides};
use crate::fl;
use crate::host::Host;
use crate::icons::{self, Illustration};
use crate::index::{AppEntry, FsView, MimeLists, Registry};
use crate::profiles::{Env, shown_icon};
use crate::setup;

/// Where the app's source and issues live.
pub(super) const REPO_URL: &str = "https://github.com/lkbddh/signpost";
const LICENSE_URL: &str = "https://www.gnu.org/licenses/gpl-3.0.html";
const TEST_LINK: &str = "https://example.org/";
const WINDOW_SIZE: Size = Size::new(640.0, 600.0);
const MIN_SIZE: Size = Size::new(360.0, 400.0);
/// Below this width the pages keep the narrow side insets.
const CONDENSED_BELOW: f32 = 480.0;

/// The settings window's surface. `exit_on_close_request` stays true: libcosmic reports a toplevel only
/// once it is closed, so the window closes itself, and closing it never ends the daemon.
#[must_use]
pub fn window_settings() -> window::Settings {
    let mut settings = window::Settings {
        size: WINDOW_SIZE,
        min_size: Some(MIN_SIZE),
        decorations: false,
        transparent: true,
        ..Default::default()
    };
    APP_ID.clone_into(&mut settings.platform_specific.application_id);
    settings
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Main,
    Shortcuts,
    About,
}

/// A link the window asks the picker queue to present.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    Test,
    Source,
    Issues,
    License,
}

impl Link {
    #[must_use]
    pub fn uri(self) -> String {
        match self {
            Self::Test => TEST_LINK.to_owned(),
            Self::Source => REPO_URL.to_owned(),
            Self::Issues => format!("{REPO_URL}/issues"),
            Self::License => LICENSE_URL.to_owned(),
        }
    }
}

/// What the header bar's window controls ask of this window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Titlebar {
    Close,
    Drag,
    Maximize,
    Minimize,
    Menu,
}

#[derive(Debug, Clone)]
pub enum Message {
    Go(Page),
    Run(Action),
    Undo(Action),
    Done {
        id: OpId,
        op: Op,
        result: Result<(), OpError>,
    },
    ToggleDetails,
    CloseToast(ToastId),
    /// A toast timer's message, acted on only while the toast of that operation is the one showing.
    Expiry(OpId, Box<Message>),
    OpenLink(Link),
    OpenAppDrawer(String),
    CloseAppDrawer,
    Swatch(Swatch),
    Titlebar(Titlebar),
    Resized(window::Id, Size),
    Maximized(bool),
    Surface(cosmic::surface::Action<Message>),
}

impl Message {
    /// The link this message asks the picker queue to present.
    #[must_use]
    pub fn link(&self) -> Option<String> {
        let Self::OpenLink(link) = self else {
            return None;
        };
        Some(link.uri())
    }
}

/// One run of an operation. Its completion names it, so the end of an older run is told from the one
/// in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpId(u64);

#[derive(Debug, Clone, Copy)]
struct Running {
    id: OpId,
    op: Op,
}

/// The operation in flight. It belongs to the daemon, not to a window: closing the window neither stops the
/// operation nor lets the next one overlap it.
#[derive(Debug, Default)]
pub struct Operations {
    issued: Cell<u64>,
    running: Cell<Option<Running>>,
}

impl Operations {
    fn running(&self) -> Option<Op> {
        self.running.get().map(|running| running.op)
    }

    /// Starts `op`, unless one is already in flight.
    fn begin(&self, op: Op) -> Option<OpId> {
        if self.running.get().is_some() {
            return None;
        }
        let id = self.mint();
        self.running.set(Some(Running { id, op }));
        Some(id)
    }

    /// An id no other has, for a toast that no operation's result owns.
    fn mint(&self) -> OpId {
        let id = OpId(self.issued.get() + 1);
        self.issued.set(id.0);
        id
    }

    /// `message`, unless it ends an operation that is not the one in flight: that is dropped. The one in
    /// flight is freed whether or not a window is open to show its result.
    #[must_use]
    pub fn settle(&self, message: Message) -> Option<Message> {
        let Message::Done { id, .. } = &message else {
            return Some(message);
        };
        if self.running.get().is_none_or(|running| running.id != *id) {
            return None;
        }
        self.running.set(None);
        Some(message)
    }
}

/// What the window reads when it refreshes and what its operations touch.
pub struct Sources<'a> {
    pub registry: &'a Registry,
    pub ops: &'a Operations,
    pub env: Env<'a>,
    pub state_dir: &'a Path,
    /// Every `mimeapps.list`, highest precedence first, where this process reads them; read again on every refresh.
    pub lists: Vec<PathBuf>,
    /// The list setup writes to, where this process writes it; `None` when a Flatpak cannot reach the host's.
    pub user_list: Option<PathBuf>,
    /// Where Signpost runs, which decides how a path read here is shown to the user.
    pub host: &'a Host,
    /// Where color choices are saved; `None` when the config directory is unavailable.
    pub color_store: Option<&'a ColorStore>,
}

/// The paths an operation needs, owned so it can run off the UI thread.
struct Files {
    user_list: Option<PathBuf>,
    lists: Vec<PathBuf>,
    state_dir: PathBuf,
}

fn execute(op: Op, files: &Files) -> Result<(), OpError> {
    let result = match op {
        Op::Set => {
            let Some(user_list) = &files.user_list else {
                return Err(OpError::Failed(fl!("setup-list-unreachable")));
            };
            setup::set_default_browser(user_list, &files.lists, &files.state_dir)
        }
        Op::Restore => setup::restore_default_browser(&files.lists, &files.state_dir),
    };
    result.map_err(|error| OpError::from(&error))
}

/// Runs `work` off the UI thread. A panic in it is reported like any failure, so the operation always
/// ends and the ledger is freed.
async fn run_blocking(
    work: impl FnOnce() -> Result<(), OpError> + Send + 'static,
) -> Result<(), OpError> {
    tokio::task::spawn_blocking(work)
        .await
        .unwrap_or(Err(OpError::Task))
}

fn identity(registry: &Registry, id: &str, view: &dyn FsView) -> Option<Identity> {
    let entry = registry.get(id)?;
    Some(Identity {
        id: id.to_owned(),
        name: entry.name.clone(),
        icon: shown_icon(entry.icon.as_deref(), view),
    })
}

/// The inventory and the page model, read from disk and the index; never per redraw.
fn facts(sources: &Sources<'_>, last: Option<&(Op, Result<(), OpError>)>) -> (Vec<AppRow>, Model) {
    let mut snapshot = setup::snapshot(&sources.lists, sources.state_dir);
    // The lists are read where this process sees them; the user sees the host's paths.
    for handler in [&mut snapshot.http, &mut snapshot.https] {
        handler.source = handler
            .source
            .as_deref()
            .map(|read| sources.host.shown(read));
    }
    let mime = MimeLists::load(&sources.lists);
    let apps: Vec<AppEntry> = sources
        .registry
        .link_candidates(setup::HTTPS, &mime)
        .into_iter()
        .cloned()
        .collect();
    let describe = |id: &str| identity(sources.registry, id, sources.env.view);
    (
        inventory(&apps, &sources.env),
        model(&snapshot, &describe, last),
    )
}

fn no_toasts() -> Toasts<Message> {
    Toasts::new(Message::CloseToast)
}

fn toast_widget(toast: Toast) -> widget::Toast<Message> {
    let widget = widget::Toast::new(toast.text);
    let Some(action) = toast.undo else {
        return widget;
    };
    widget.action(fl!("undo"), move |_| Message::Undo(action))
}

pub struct Window {
    id: window::Id,
    pub(super) page: Page,
    pub(super) condensed: bool,
    pub(super) maximized: bool,
    pub(super) inventory: Vec<AppRow>,
    pub(super) model: Model,
    /// The newest operation and its result; an earlier failure stays until a later one replaces it.
    last: Option<(Op, Result<(), OpError>)>,
    pub(super) details_open: bool,
    /// The operation in flight, as the daemon's [`Operations`] holds it; no other may overlap.
    pending: Option<Op>,
    /// The app whose drawer is open.
    pub(super) drawer: Option<Drawer>,
    /// The About hero drawn for the theme's tone, and the sky it fades in with.
    pub(super) hero: svg::Handle,
    pub(super) hero_sky: Color,
    pub(crate) toasts: Toasts<Message>,
    /// The operation whose result `toasts` show; only a timer that operation started may close them.
    toast_op: Option<OpId>,
}

impl Window {
    /// A window on the main page, with the page read from `sources`.
    #[must_use]
    pub fn open(id: window::Id, dark: bool, sources: &Sources<'_>) -> Self {
        let (inventory, model) = facts(sources, None);
        Self {
            id,
            page: Page::Main,
            condensed: false,
            maximized: false,
            inventory,
            model,
            last: None,
            details_open: false,
            pending: sources.ops.running(),
            drawer: None,
            hero: icons::illustration(Illustration::AboutHero, dark),
            hero_sky: icons::sky(Illustration::AboutHero, dark),
            toasts: no_toasts(),
            toast_op: None,
        }
    }

    #[must_use]
    pub fn id(&self) -> window::Id {
        self.id
    }

    /// Whether the window is maximized, as the compositor last reported.
    #[must_use]
    pub fn is_maximized(&self) -> bool {
        self.maximized
    }

    #[cfg(test)]
    pub(crate) fn model(&self) -> &Model {
        &self.model
    }

    pub(super) fn is_busy(&self) -> bool {
        self.pending.is_some()
    }

    /// `slot` loses its window when `id` is that window's; true when it did.
    pub fn closed(slot: &mut Option<Self>, id: window::Id) -> bool {
        slot.take_if(|window| window.id == id).is_some()
    }

    /// Reads the files and the index again; an open drawer follows its app.
    pub fn refresh(&mut self, sources: &Sources<'_>) {
        (self.inventory, self.model) = facts(sources, self.last.as_ref());
        self.drawer = self
            .drawer
            .take()
            .and_then(|drawer| drawer.follow(&self.inventory));
    }

    /// What Esc asks of this window: closing the color popover, while one is open.
    #[must_use]
    pub fn dismiss(&self, id: window::Id, event: &keyboard::Event) -> Option<Message> {
        let pressed = matches!(
            event,
            keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(Named::Escape),
                ..
            }
        );
        let open = self
            .drawer
            .as_ref()
            .is_some_and(|drawer| drawer.swatch.is_some());
        (id == self.id && pressed && open).then_some(Message::Swatch(Swatch::Dismiss))
    }

    /// Says that too many links are waiting, in place of the toast showing.
    pub fn links_waiting(&mut self, sources: &Sources<'_>) -> Task<Message> {
        self.clear_toasts();
        let toast = Toast {
            text: fl!("toast-links-waiting"),
            undo: None,
        };
        self.show(sources.ops.mint(), toast)
    }

    pub fn retheme(&mut self, dark: bool) {
        self.hero = icons::illustration(Illustration::AboutHero, dark);
        self.hero_sky = icons::sky(Illustration::AboutHero, dark);
    }

    pub fn update(
        &mut self,
        message: Message,
        sources: &Sources<'_>,
        colors: &mut Overrides,
    ) -> Task<Message> {
        match message {
            Message::Go(page) => {
                self.page = page;
                self.drawer = None;
                Task::none()
            }
            Message::Run(action) => self.run(action.op(), sources),
            Message::Undo(action) => {
                self.clear_toasts();
                self.run(action.op(), sources)
            }
            Message::Done { id, op, result } => self.finish(id, op, result, sources),
            Message::ToggleDetails => {
                self.details_open = !self.details_open;
                Task::none()
            }
            Message::CloseToast(id) => {
                self.toasts.remove(id);
                Task::none()
            }
            Message::Expiry(op, close) if self.toast_op == Some(op) => {
                self.update(*close, sources, colors)
            }
            Message::Expiry(..) | Message::OpenLink(_) => Task::none(),
            Message::OpenAppDrawer(app) => {
                self.open_drawer(app);
                Task::none()
            }
            Message::CloseAppDrawer => {
                self.drawer = None;
                Task::none()
            }
            Message::Swatch(swatch) => {
                if let Some(drawer) = &mut self.drawer {
                    drawer.update(swatch, sources.color_store, colors);
                }
                Task::none()
            }
            Message::Titlebar(action) => self.titlebar(action),
            Message::Resized(id, size) => self.resized(id, size),
            Message::Maximized(maximized) => {
                self.maximized = maximized;
                Task::none()
            }
            Message::Surface(action) => cosmic::surface::surface_task(action),
        }
    }

    /// A press on an app that has left the list since the page was drawn opens nothing.
    fn open_drawer(&mut self, app: String) {
        if self.inventory.iter().any(|row| row.id == app) {
            self.drawer = Some(Drawer::open(app));
        }
    }

    fn run(&mut self, op: Op, sources: &Sources<'_>) -> Task<Message> {
        self.start(op, sources, execute)
    }

    /// Begins `op` and has `work` do it off the UI thread; the tests hand it work that fails.
    fn start(
        &mut self,
        op: Op,
        sources: &Sources<'_>,
        work: fn(Op, &Files) -> Result<(), OpError>,
    ) -> Task<Message> {
        let Some(id) = sources.ops.begin(op) else {
            return Task::none();
        };
        self.pending = Some(op);
        let files = Files {
            user_list: sources.user_list.clone(),
            lists: sources.lists.clone(),
            state_dir: sources.state_dir.to_owned(),
        };
        Task::perform(run_blocking(move || work(op, &files)), move |result| {
            cosmic::Action::App(Message::Done { id, op, result })
        })
    }

    fn finish(
        &mut self,
        id: OpId,
        op: Op,
        result: Result<(), OpError>,
        sources: &Sources<'_>,
    ) -> Task<Message> {
        self.pending = None;
        self.last = Some((op, result));
        self.details_open = false;
        self.refresh(sources);
        self.clear_toasts();
        let Some(toast) = self.model.toast.clone() else {
            return Task::none();
        };
        self.show(id, toast)
    }

    /// Shows `toast`, which only the timer it starts may close.
    fn show(&mut self, id: OpId, toast: Toast) -> Task<Message> {
        self.toast_op = Some(id);
        self.toasts
            .push(toast_widget(toast))
            .map(move |closed| cosmic::Action::App(Message::Expiry(id, Box::new(closed))))
    }

    fn clear_toasts(&mut self) {
        self.toasts = no_toasts();
        self.toast_op = None;
    }

    fn titlebar(&self, action: Titlebar) -> Task<Message> {
        let id = self.id;
        match action {
            Titlebar::Close => window::close(id),
            Titlebar::Drag => window::drag(id),
            Titlebar::Maximize => window::toggle_maximize(id),
            Titlebar::Minimize => window::minimize(id, true),
            Titlebar::Menu => window::show_system_menu(id),
        }
    }

    fn resized(&mut self, id: window::Id, size: Size) -> Task<Message> {
        if id != self.id {
            return Task::none();
        }
        self.condensed = size.width < CONDENSED_BELOW;
        window::is_maximized(id).map(|maximized| cosmic::Action::App(Message::Maximized(maximized)))
    }
}

#[cfg(test)]
mod tests;
