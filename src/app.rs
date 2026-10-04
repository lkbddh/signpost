use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cosmic::app::{Core, Settings, Task};
use cosmic::cctk::sctk::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer};
use cosmic::iced::event::PlatformSpecific;
use cosmic::iced::event::wayland::{Event as WaylandEvent, LayerEvent};
use cosmic::iced::keyboard;
use cosmic::iced::platform_specific::shell::commands::activation;
use cosmic::iced::runtime::platform_specific::wayland::layer_surface::{
    IcedOutput, SctkLayerSurfaceSettings,
};
use cosmic::iced::{Event, Subscription, event, mouse, window};
use cosmic::{ApplicationExt, Element, widget};

use crate::bus::{APP_ID, BusEvent, Daemon, Readiness};
use crate::frost::{self, Frost};
use crate::host::Host;
use crate::index::{MimeLists, Registry};
use crate::picker::{
    Backlog, COPIED_FLASH, Input, LONG_PRESS, LaunchFailure, LinkQueue, Outcome, Picker,
    QueuedLink, early_failure, map_key,
};
use crate::profiles::{Env, link_tiles};
use crate::{colors, fl, picker_view, settings};

use self::launching::{Pending, PickerState};
use self::menu::MenuPopup;
use self::paths::{
    Paths, home_dir, load_colors, mime_list_paths, user_list_path, view_of, watched,
};

mod directories;
mod launching;
mod menu;
mod paths;
mod watcher;

const TOKEN_TIMEOUT: Duration = Duration::from_secs(2);
pub(crate) static PICKER_AUTOSIZE: std::sync::LazyLock<cosmic::iced::id::Id> =
    std::sync::LazyLock::new(|| cosmic::iced::id::Id::new("signpost-picker"));
/// The tile grid's viewport (one picker is presented at a time).
pub(crate) static PICKER_SCROLL: std::sync::LazyLock<cosmic::iced::id::Id> =
    std::sync::LazyLock::new(|| cosmic::iced::id::Id::new("signpost-picker-scroll"));
/// The actions menu's popup, whose surface is as big as it.
pub(crate) static MENU_AUTOSIZE: std::sync::LazyLock<cosmic::iced::id::Id> =
    std::sync::LazyLock::new(|| cosmic::iced::id::Id::new("signpost-picker-menu"));
/// The actions menu's viewport.
pub(crate) static MENU_SCROLL: std::sync::LazyLock<cosmic::iced::id::Id> =
    std::sync::LazyLock::new(|| cosmic::iced::id::Id::new("signpost-picker-menu-scroll"));

/// Only http/https URLs that parse are accepted; everything else is logged and dropped.
#[must_use]
pub fn valid_link_uris(uris: &[String]) -> Vec<String> {
    uris.iter()
        .filter(|u| {
            let valid = link_scheme(u).is_some();
            if !valid {
                // Its scheme only: a link can carry private details.
                let scheme = url::Url::parse(u)
                    .map_or_else(|_| "unparseable".to_owned(), |url| url.scheme().to_owned());
                tracing::info!(scheme, "ignoring a link that is not http(s)");
            }
            valid
        })
        .cloned()
        .collect()
}

/// The links of an `Open` call to queue. The bus counted all of them as waiting; those dropped here
/// are released.
fn accepted_links(uris: &[String], backlog: &Backlog) -> Vec<String> {
    let links = valid_link_uris(uris);
    backlog.release(uris.len() - links.len());
    links
}

/// The lowercase scheme of an http(s) link, taken from the parsed URL; `None` otherwise.
#[must_use]
pub fn link_scheme(uri: &str) -> Option<String> {
    url::Url::parse(uri)
        .ok()
        .map(|u| u.scheme().to_owned())
        .filter(|s| matches!(s.as_str(), "http" | "https"))
}

/// iced key → picker key (`c` maps to `CopyC`; Ctrl is checked by `picker::map_key`).
#[must_use]
pub fn key_from(key: &keyboard::Key) -> Option<crate::picker::Key> {
    use crate::picker::Key;
    use keyboard::key::Named;
    match key {
        keyboard::Key::Named(Named::ArrowLeft) => Some(Key::Left),
        keyboard::Key::Named(Named::ArrowRight) => Some(Key::Right),
        keyboard::Key::Named(Named::ArrowUp) => Some(Key::Up),
        keyboard::Key::Named(Named::ArrowDown) => Some(Key::Down),
        keyboard::Key::Named(Named::Enter) => Some(Key::Enter),
        keyboard::Key::Named(Named::Escape) => Some(Key::Escape),
        keyboard::Key::Named(Named::ContextMenu) => Some(Key::Menu),
        keyboard::Key::Named(Named::Tab) => Some(Key::Tab),
        keyboard::Key::Character(c) => match c.as_str() {
            "c" | "C" => Some(Key::CopyC),
            d => d
                .parse::<u8>()
                .ok()
                .filter(|n| (1..=9).contains(n))
                .map(Key::Digit),
        },
        _ => None,
    }
}

/// Which runtime events reach the picker logic: keys no widget captured (Enter on a focused
/// button has already acted), every modifier change (Ctrl tracking), every mouse press (whichever widget
/// took it, it ends the keyboard's focus), touch (that, and long-press cancellation), layer focus loss, the
/// compositor's announcement that it can blur and a window's opening. Nothing else, so an idle picker
/// produces no traffic.
#[must_use]
pub fn forward_event(event: &Event, status: event::Status) -> bool {
    match event {
        Event::Keyboard(keyboard::Event::ModifiersChanged(_))
        | Event::Mouse(mouse::Event::ButtonPressed(_))
        | Event::Touch(_)
        | Event::Window(window::Event::Opened { .. })
        | Event::PlatformSpecific(PlatformSpecific::Wayland(
            WaylandEvent::Layer(LayerEvent::Unfocused, _, _) | WaylandEvent::BlurEnabled,
        )) => true,
        Event::Keyboard(_) => status == event::Status::Ignored,
        _ => false,
    }
}

/// Touch slop: a finger that moves farther than this from where it went down cancels a long-press.
pub const TOUCH_SLOP: f32 = 12.0;

/// True when a touch has moved far enough from its origin to cancel the press (`mouse_area`'s exit
/// tracking relies on hover, which touch never establishes at this pin).
#[must_use]
pub fn touch_cancels(origin: (f32, f32), now: (f32, f32)) -> bool {
    let (dx, dy) = (now.0 - origin.0, now.1 - origin.1);
    dx.hypot(dy) > TOUCH_SLOP
}

/// Scrolls `viewport` to `fraction` of the way down.
fn snap(viewport: cosmic::iced::id::Id, fraction: f32) -> Task<Message> {
    cosmic::iced::widget::scrollable::snap_to(
        viewport,
        cosmic::iced::widget::scrollable::RelativeOffset {
            x: None,
            y: Some(fraction),
        },
    )
}

/// Scrolls keyboard focus beyond the viewport into view.
fn snap_to_focus(picker: &Picker) -> Task<Message> {
    let viewport = if picker.menu.is_some() {
        MENU_SCROLL.clone()
    } else {
        PICKER_SCROLL.clone()
    };
    snap(viewport, picker_view::focus_scroll(picker))
}

/// While the focus shows, a key that navigates or any change to it scrolls it into view.
fn reveal_focus(picker: &Picker, navigates: bool, changed: bool) -> Task<Message> {
    if picker.focus_visible && (navigates || changed) {
        return snap_to_focus(picker);
    }
    Task::none()
}

#[derive(Clone)]
pub struct Inbox(Arc<Mutex<Option<tokio::sync::mpsc::Receiver<BusEvent>>>>);

impl Hash for Inbox {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::ptr::hash(Arc::as_ptr(&self.0), state);
    }
}

pub struct Flags {
    /// Where Signpost runs: natively, or in a Flatpak sandbox with the host's environment.
    host: Host,
    inbox: Inbox,
    conn: zbus::Connection,
    /// Taken by `init`, which publishes Ready only once the app exists.
    readiness: Mutex<Option<Readiness>>,
    backlog: Backlog,
}

#[derive(Debug, Clone)]
pub enum Message {
    Bus(BusEvent),
    Event(window::Id, Event),
    Tile(window::Id, Input),
    Token {
        id: window::Id,
        attempt: u64,
        token: Option<String>,
    },
    TokenTimeout {
        id: window::Id,
        attempt: u64,
    },
    /// Launch attempt `attempt` of picker `id` completed (spawned, D-Bus `Open` answered, or failed).
    Launched {
        id: window::Id,
        attempt: u64,
        error: Option<String>,
    },
    EarlyFailure {
        source: window::Id,
        attempt: u64,
        failure: LaunchFailure,
    },
    IndexChanged,
    Settings(settings::Message),
    /// Where tile `tile` of picker `id` is, for the popup of its menu; `None` when no tile answered.
    MenuAnchored {
        id: window::Id,
        tile: usize,
        bounds: Option<cosmic::iced::Rectangle>,
    },
    /// A surface is gone (libcosmic reports this after destruction, also for compositor-closed layers).
    SurfaceClosed(window::Id),
    /// What the open surfaces ask of libcosmic changed, or a surface came or went.
    SyncSurfaces,
    Shutdown,
}

pub struct App {
    core: Core,
    host: Host,
    conn: zbus::Connection,
    inbox: Inbox,
    paths: Paths,
    registry: Registry,
    lists: MimeLists,
    pickers: HashMap<window::Id, PickerState>,
    ctrl: bool,
    attempt: u64,
    settings: Option<settings::Window>,
    /// Whether the runtime has yet to report the settings window's surface; it ignores activation until then.
    settings_opening: bool,
    /// The latest token the opening settings window was asked to raise with, used once its surface exists.
    settings_token: Option<String>,
    /// The setup operation in flight; it outlives the settings window.
    operations: settings::Operations,
    queue: LinkQueue,
    /// The picker closing: no other is presented until its surface is gone.
    after_close: Option<window::Id>,
    /// The links accepted over the bus or queued here that no picker has presented yet.
    backlog: Backlog,
    colors: colors::Overrides,
    /// Where the choices in `colors` are written; `None` when the config directory is unavailable.
    color_store: Option<colors::ColorStore>,
    frost: Frost,
    menu_popup: Option<MenuPopup>,
}

impl App {
    fn reload_index(&mut self) {
        let locales = cosmic::desktop::fde::get_languages_from_env();
        self.registry = match &self.host {
            Host::Native => Registry::load(&self.paths.data_dirs, &locales),
            Host::Flatpak(env) => Registry::load_in(env, &self.paths.data_dirs, &locales),
        };
        self.lists = MimeLists::load(&mime_list_paths(&self.host));
    }

    fn open_picker(&mut self, link: QueuedLink) -> Task<Message> {
        // Every queued link passed `valid_link_uris`; the original string is what gets launched.
        let scheme = link_scheme(&link.uri).unwrap_or_else(|| "https".to_owned());
        let mime = format!("x-scheme-handler/{scheme}");
        let env = Env {
            home: &self.paths.home,
            config_home: &self.paths.config_home,
            view: view_of(&self.host),
        };
        let apps: Vec<crate::index::AppEntry> = self
            .registry
            .link_candidates(&mime, &self.lists)
            .into_iter()
            .cloned()
            .collect();
        let picker = Picker::from_link(link, link_tiles(&apps, &env));
        let id = window::Id::unique();
        self.pickers.insert(id, PickerState::new(picker, apps));
        let surface = cosmic::surface::action::app_layer_shell::<Self>(
            |app| frost::live_settings(app.frost.picker()),
            move |_| SctkLayerSurfaceSettings {
                id,
                layer: Layer::Overlay,
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
                anchor: Anchor::empty(),
                output: IcedOutput::Active,
                namespace: format!("{APP_ID}.Picker"),
                size: None,
                ..Default::default()
            },
            Some(Box::new(move |app: &App| {
                app.picker_view(id).map(cosmic::Action::App)
            })),
        );
        cosmic::surface::surface_task(surface)
    }

    fn settings_frosted(&self) -> bool {
        let maximized = self
            .settings
            .as_ref()
            .is_some_and(settings::Window::is_maximized);
        self.frost.settings(maximized)
    }

    /// Each open picker and the settings window, in a fixed order, with whether it asks to be blurred.
    fn asked_blur(&self) -> Vec<(window::Id, bool)> {
        let pickers = self.pickers.keys().map(|&id| (id, self.frost.picker()));
        let settings = self
            .settings
            .as_ref()
            .map(|window| (window.id(), self.settings_frosted()));
        let mut asked: Vec<_> = pickers.chain(settings).collect();
        asked.sort();
        asked
    }

    /// Has libcosmic read every open surface's live settings again.
    fn sync_surfaces(&self) -> Task<Message> {
        Task::batch(self.asked_blur().into_iter().map(|(id, _)| {
            cosmic::surface::surface_task(cosmic::surface::Action::SyncLiveSettings(id))
        }))
    }

    fn close_picker(&mut self, id: window::Id) -> Task<Message> {
        // A late Launched/EarlyFailure for an already-closed picker must not advance the queue again.
        if !self.pickers.contains_key(&id) {
            return Task::none();
        }
        // Before it goes: a popup its launch holds is kept with it.
        let popups = self.destroy_popups(id);
        self.pickers.remove(&id);
        let destroy = popups.chain(cosmic::surface::surface_task(
            cosmic::surface::action::destroy_layer_shell(id),
        ));
        // The next picker waits until this one's surface is gone, so two never show at once.
        self.queue.closing();
        self.after_close = Some(id);
        destroy
    }

    /// Presents the link that waited for picker `closed` to leave the screen, if one did.
    fn present_after(&mut self, closed: window::Id) -> Task<Message> {
        if self.after_close.take_if(|id| *id == closed).is_none() {
            return Task::none();
        }
        let next = self.queue.gone();
        self.present(next)
    }

    /// A link that has just come in: present now if nothing is showing, otherwise queue;
    /// the same link sent twice at once is one. The link is already counted as waiting.
    fn enqueue(&mut self, link: QueuedLink) -> Task<Message> {
        let now = self.queue.arrive(link, std::time::Instant::now());
        self.present(now)
    }

    /// [`App::enqueue`] for a link whose picker closed on a failure, never taken for a repeat. It was
    /// counted out when it was presented, so it only takes the room it left.
    fn enqueue_own(&mut self, link: QueuedLink) -> Task<Message> {
        self.backlog.add(1);
        let now = self.queue.push(link);
        self.present(now)
    }

    fn present(&mut self, link: Option<QueuedLink>) -> Task<Message> {
        link.map_or_else(Task::none, |now| self.open_picker(now))
    }

    /// Queues a link the settings window asks to open, or has the window say that too many wait.
    fn settings_link(&mut self, uri: String) -> Task<Message> {
        if self.backlog.try_add(1) {
            return self.enqueue(QueuedLink::new(uri));
        }
        self.with_settings(settings::Window::links_waiting)
            .map_or_else(Task::none, |task| {
                task.map(|action| action.map(Message::Settings))
            })
    }

    fn apply(&mut self, id: window::Id, input: Input) -> Task<Message> {
        let Some(picker) = self.pickers.get_mut(&id).map(|open| &mut open.picker) else {
            return Task::none();
        };
        let navigates = input.is_navigation();
        let focus_state = |picker: &Picker| {
            (
                picker.focus,
                picker.menu,
                picker.menu_focus,
                picker.focus_visible,
            )
        };
        let before = focus_state(picker);
        let menu_before = picker.menu;
        let outcome = picker.handle(input);
        let reveal = reveal_focus(picker, navigates, focus_state(picker) != before);
        // A menu that opens takes the native focus whoever opened it, so that Enter acts on the menu and
        // not on a control the pointer left focused.
        let opens_menu = menu_before.is_none() && picker.menu.is_some();
        let claims_focus = (picker.focus_visible && navigates) || opens_menu;
        let claim = claims_focus.then(|| self.clear_native_focus(id));
        let popup = self.claim_popup(id, &outcome);
        let menu = self.follow_menu(id, menu_before);
        let task = match outcome {
            Outcome::None => Task::none(),
            Outcome::Cancel => self.close_picker(id),
            Outcome::Copy { uri, seq } => Task::batch([
                cosmic::iced::clipboard::write(uri),
                Task::perform(tokio::time::sleep(COPIED_FLASH), move |()| {
                    cosmic::Action::App(Message::Tile(id, Input::CopiedExpired(seq)))
                }),
            ]),
            Outcome::ArmLongPress(seq) => {
                Task::perform(tokio::time::sleep(LONG_PRESS), move |()| {
                    cosmic::Action::App(Message::Tile(id, Input::LongPress(seq)))
                })
            }
            Outcome::Launch {
                index,
                action,
                keep_open,
            } => {
                let attempt = self.attempt + 1;
                let pending = Pending {
                    attempt,
                    index,
                    action,
                    keep_open,
                    launching: false,
                    popup,
                };
                let admitted = self
                    .pickers
                    .get_mut(&id)
                    .is_some_and(|open| open.admit(pending));
                if admitted {
                    self.attempt = attempt;
                    let window = popup.unwrap_or(id);
                    let token = activation::request_token(Some(APP_ID.to_owned()), Some(window))
                        .map(move |token| {
                            cosmic::Action::App(Message::Token { id, attempt, token })
                        });
                    let timeout = Task::perform(tokio::time::sleep(TOKEN_TIMEOUT), move |()| {
                        cosmic::Action::App(Message::TokenTimeout { id, attempt })
                    });
                    Task::batch([token, timeout])
                } else {
                    tracing::debug!(
                        "a launch is already pending in this picker; selection ignored"
                    );
                    popup.map_or_else(Task::none, menu::destroy)
                }
            }
        };
        Task::batch(claim.into_iter().chain([reveal, menu, task]))
    }

    fn key(&mut self, surface: window::Id, event: keyboard::Event) -> Task<Message> {
        let id = self.picker_of(surface);
        if let Some(message) = self
            .settings
            .as_ref()
            .and_then(|window| window.dismiss(id, &event))
        {
            return self.settings_update(message);
        }
        let input = match event {
            keyboard::Event::ModifiersChanged(m) => {
                self.ctrl = m.control();
                None
            }
            keyboard::Event::KeyPressed { key, modifiers, .. } => {
                self.ctrl = modifiers.control();
                key_from(&key)
                    .and_then(|k| map_key(k, modifiers.control(), true))
                    .map(|input| input.with_shift(modifiers.shift()))
            }
            keyboard::Event::KeyReleased { key, modifiers, .. } => {
                key_from(&key).and_then(|k| map_key(k, modifiers.control(), false))
            }
        };
        if matches!(input, Some(Input::Escape))
            && let Some(dismissed) = self.dismiss_held_popup(surface)
        {
            return dismissed;
        }
        match input {
            Some(input) => self.apply(id, input),
            None => Task::none(),
        }
    }

    fn touch(&mut self, id: window::Id, touch: cosmic::iced::touch::Event) -> Task<Message> {
        use cosmic::iced::touch::Event as T;
        let Some(state) = self.pickers.get_mut(&id) else {
            return Task::none();
        };
        match touch {
            T::FingerPressed {
                id: finger,
                position,
            } => {
                state.touch = Some((finger, (position.x, position.y)));
                self.apply(id, Input::PointerDown)
            }
            T::FingerMoved {
                id: finger,
                position,
            } => match state.touch {
                Some((f, origin))
                    if f == finger && touch_cancels(origin, (position.x, position.y)) =>
                {
                    state.touch = None;
                    self.apply(id, Input::PressCancel)
                }
                _ => Task::none(),
            },
            T::FingerLost { .. } => {
                state.touch = None;
                self.apply(id, Input::PressCancel)
            }
            T::FingerLifted { .. } => {
                state.touch = None;
                Task::none()
            }
        }
    }

    /// Picker `id`'s model, while it is open.
    fn picker(&self, id: window::Id) -> Option<&Picker> {
        self.pickers.get(&id).map(|open| &open.picker)
    }

    fn picker_mut(&mut self, id: window::Id) -> Option<&mut Picker> {
        self.pickers.get_mut(&id).map(|open| &mut open.picker)
    }

    fn picker_view(&self, id: window::Id) -> Element<'_, Message> {
        let Some(picker) = self.picker(id) else {
            return widget::Space::new().into();
        };
        let theme = cosmic::theme::active();
        let view = self.view_of(id, picker, &theme).render();
        frost::scoped(theme, self.frost.picker(), view)
    }

    /// What picker `id`'s view reads, under `theme`.
    fn view_of<'a>(
        &'a self,
        id: window::Id,
        picker: &'a Picker,
        theme: &cosmic::Theme,
    ) -> picker_view::View<'a> {
        picker_view::View {
            id,
            picker,
            colors: &self.colors,
            queue: &self.queue,
            dark: theme.cosmic().is_dark,
            radius: theme.cosmic().radius_m(),
        }
    }

    fn settings_sources(&self) -> settings::Sources<'_> {
        settings::Sources {
            registry: &self.registry,
            ops: &self.operations,
            env: Env {
                home: &self.paths.home,
                config_home: &self.paths.config_home,
                view: view_of(&self.host),
            },
            state_dir: &self.paths.state_home,
            lists: mime_list_paths(&self.host),
            user_list: user_list_path(&self.host, &self.paths),
            host: &self.host,
            color_store: self.color_store.as_ref(),
        }
    }

    /// Runs `f` on the settings window, if one is open, with what it reads. The window is out of `self`
    /// meanwhile, so the sources can borrow the rest of the app.
    fn with_settings<R>(
        &mut self,
        f: impl FnOnce(&mut settings::Window, &settings::Sources<'_>) -> R,
    ) -> Option<R> {
        let mut window = self.settings.take()?;
        let result = f(&mut window, &self.settings_sources());
        self.settings = Some(window);
        Some(result)
    }

    /// Raises the settings window with the token kept while it opened, now that its surface exists.
    fn settings_opened(&mut self, id: window::Id) -> Task<Message> {
        if self.settings.as_ref().map(settings::Window::id) != Some(id) {
            return Task::none();
        }
        self.settings_opening = false;
        self.settings_token
            .take()
            .map_or_else(Task::none, |token| activation::activate(id, token))
    }

    fn settings_update(&mut self, message: settings::Message) -> Task<Message> {
        let link = message.link().map(|uri| self.settings_link(uri));
        // Out of `self` while the window changes it, so the sources can borrow the rest of the app.
        let mut colors = std::mem::take(&mut self.colors);
        let task = self
            .operations
            .settle(message)
            .and_then(|message| {
                self.with_settings(|window, sources| window.update(message, sources, &mut colors))
            })
            .map(|task| task.map(|action| action.map(Message::Settings)));
        self.colors = colors;
        Task::batch(link.into_iter().chain(task))
    }

    fn open_settings(&mut self, token: Option<String>) -> Task<Message> {
        if let Some(window) = &self.settings {
            let id = window.id();
            if self.settings_opening {
                self.settings_token = token.or(self.settings_token.take());
                return Task::none();
            }
            // Nothing watches the record of the saved defaults, so a repair shows when the window is raised.
            self.with_settings(settings::Window::refresh);
            return token.map_or_else(
                || window::gain_focus(id),
                |token| activation::activate(id, token),
            );
        }
        self.settings_opening = true;
        self.settings_token = token;
        let (id, open) = cosmic::surface::action::app_window::<Self>(
            |app| frost::live_settings(app.settings_frosted()),
            |_| settings::window_settings(),
            None,
        );
        self.settings = Some(settings::Window::open(
            id,
            cosmic::theme::is_dark(),
            &self.settings_sources(),
        ));
        // iced reads the title from `core.title` when it processes the open task, so it is set
        // before that task runs; the xdg_toplevel title is what COSMIC shows in the app switcher.
        let title = self.set_window_title(fl!("app-name"), id);
        Task::batch([title, cosmic::surface::surface_task(open)])
    }
}

impl cosmic::Application for App {
    type Executor = cosmic::executor::Default;
    type Flags = Flags;
    type Message = Message;
    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(mut core: Core, flags: Flags) -> (Self, Task<Message>) {
        frost::own_layer_blur(&mut core);
        let paths = Paths::for_host(&flags.host);
        let (color_store, colors) = load_colors(&flags.host, &paths.own_data);
        let mut app = Self {
            core,
            host: flags.host,
            conn: flags.conn,
            inbox: flags.inbox,
            paths,
            registry: Registry::default(),
            lists: MimeLists::load(&[]),
            pickers: HashMap::new(),
            ctrl: false,
            attempt: 0,
            settings: None,
            settings_opening: false,
            settings_token: None,
            operations: settings::Operations::default(),
            queue: LinkQueue::counting(flags.backlog.clone()),
            after_close: None,
            backlog: flags.backlog,
            colors,
            color_store,
            frost: Frost::default().following(cosmic::theme::active().cosmic()),
            menu_popup: None,
        };
        tracing::debug!(host = ?app.host, "where signpost runs");
        crate::launch::exec::read_shells_from(&app.host);
        app.reload_index();
        // The app exists and the Wayland preflight passed (lib::run): now, and only now, admit work.
        if let Some(readiness) = flags.readiness.lock().ok().and_then(|mut g| g.take()) {
            readiness.mark_ready();
        }
        (app, Task::none())
    }

    fn subscription(&self) -> Subscription<Message> {
        let bus = Subscription::run_with(self.inbox.clone(), |inbox| {
            let rx = inbox.0.lock().ok().and_then(|mut g| g.take());
            cosmic::iced::stream::channel(
                16,
                move |mut out: cosmic::iced::futures::channel::mpsc::Sender<Message>| async move {
                    use cosmic::iced::futures::SinkExt;
                    if let Some(mut rx) = rx {
                        while let Some(ev) = rx.recv().await {
                            if out.send(Message::Bus(ev)).await.is_err() {
                                return;
                            }
                        }
                    }
                    std::future::pending::<()>().await;
                },
            )
        });
        // Filtered by `forward_event`: no idle update/log flood.
        let input = event::listen_with(|event, status, id| {
            forward_event(&event, status).then(|| Message::Event(id, event))
        });
        let watcher = Subscription::run_with(watched(&self.host, &self.paths), |wanted| {
            watcher::index_changes(wanted.clone())
        });
        let shutdown = Subscription::run(|| {
            cosmic::iced::stream::channel(
                1,
                |mut out: cosmic::iced::futures::channel::mpsc::Sender<Message>| async move {
                    use cosmic::iced::futures::SinkExt;
                    if let Ok(mut term) =
                        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    {
                        term.recv().await;
                        let _ = out.send(Message::Shutdown).await;
                    }
                    std::future::pending::<()>().await;
                },
            )
        });
        let resized = window::resize_events()
            .map(|(id, size)| Message::Settings(settings::Message::Resized(id, size)));
        // One message each time a surface opens or closes or what it asks for changes; libcosmic drops the
        // task its system theme hook returns.
        let surfaces = Subscription::run_with(self.asked_blur(), |_| {
            futures_util::stream::once(std::future::ready(Message::SyncSurfaces))
        });
        Subscription::batch([bus, input, watcher, shutdown, resized, surfaces])
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Bus(BusEvent::Open { uris }) => {
                let tasks: Vec<Task<Message>> = accepted_links(&uris, &self.backlog)
                    .into_iter()
                    .map(|uri| self.enqueue(QueuedLink::new(uri)))
                    .collect();
                Task::batch(tasks)
            }
            Message::Bus(BusEvent::Activate { token }) => self.open_settings(token),
            Message::Event(id, Event::Window(window::Event::Opened { .. })) => {
                self.settings_opened(id)
            }
            Message::Event(id, Event::Keyboard(event)) => self.key(id, event),
            Message::Event(
                _,
                Event::PlatformSpecific(PlatformSpecific::Wayland(WaylandEvent::Layer(
                    LayerEvent::Unfocused,
                    _,
                    layer_id,
                ))),
            ) => self.apply(layer_id, Input::FocusLost),
            Message::Event(id, Event::Mouse(mouse::Event::ButtonPressed(_))) => {
                self.apply(self.picker_of(id), Input::PointerDown)
            }
            Message::Event(id, Event::Touch(touch)) => self.touch(self.picker_of(id), touch),
            Message::Event(
                _,
                Event::PlatformSpecific(PlatformSpecific::Wayland(WaylandEvent::BlurEnabled)),
            ) => {
                self.frost = self.frost.supported();
                Task::none()
            }
            Message::Event(..) => Task::none(),
            Message::Tile(id, input) => self.apply(id, input.with_ctrl(self.ctrl)),
            Message::Token { id, attempt, token } => self.launch(id, attempt, token),
            Message::TokenTimeout { id, attempt } => {
                if !self
                    .pickers
                    .get(&id)
                    .is_some_and(|s| s.awaiting_token(attempt))
                {
                    return Task::none();
                }
                tracing::info!("activation token timed out; launching without one");
                self.launch(id, attempt, None)
            }
            Message::Launched { id, attempt, error } => self.launched(id, attempt, error),
            Message::EarlyFailure {
                source,
                attempt,
                failure,
            } => match early_failure(self.picker_mut(source), failure) {
                None => {
                    tracing::info!(attempt, "launch failed early; shown in the open picker");
                    self.picker(source).map_or_else(Task::none, snap_to_focus)
                }
                // It closed after launching: re-present that link with the diagnostic (queued if busy).
                Some(link) => self.enqueue_own(link),
            },
            Message::IndexChanged => {
                self.reload_index();
                self.with_settings(settings::Window::refresh);
                Task::none()
            }
            Message::Settings(message) => self.settings_update(message),
            Message::MenuAnchored { id, tile, bounds } => self.menu_anchored(id, tile, bounds),
            Message::SurfaceClosed(id) => {
                if settings::Window::closed(&mut self.settings, id) {
                    return Task::none();
                }
                if let Some(closed) = self.menu_popup_closed(id) {
                    return closed;
                }
                // A picker the compositor closed must still advance the queue; for one we destroyed
                // ourselves `close_picker` is a no-op. Either way its surface is gone: the next may show.
                let closing = self.close_picker(id);
                closing.chain(self.present_after(id))
            }
            Message::SyncSurfaces => self.sync_surfaces(),
            Message::Shutdown => {
                self.queue.discard_waiting();
                self.after_close = None;
                let open: Vec<window::Id> = self.pickers.keys().copied().collect();
                let closes: Vec<Task<Message>> =
                    open.into_iter().map(|id| self.close_picker(id)).collect();
                Task::batch(closes).chain(cosmic::iced::exit())
            }
        }
    }

    fn on_close_requested(&self, id: window::Id) -> Option<Message> {
        // At this pin libcosmic calls this once a surface is gone (`window::Event::Closed` or
        // `LayerEvent::Done`), not on a close request.
        Some(Message::SurfaceClosed(id))
    }

    fn view(&self) -> Element<'_, Message> {
        widget::Space::new().into()
    }

    fn system_theme_update(
        &mut self,
        _keys: &[&'static str],
        theme: &cosmic::cosmic_theme::Theme,
    ) -> Task<Message> {
        self.frost = self.frost.following(theme);
        if let Some(window) = &mut self.settings {
            window.retheme(theme.is_dark);
        }
        Task::none()
    }

    fn view_window(&self, id: window::Id) -> Element<'_, Message> {
        let Some(window) = self.settings.as_ref().filter(|window| window.id() == id) else {
            return self.picker_view(id);
        };
        let view = window
            .view(self.core.focused_window() == Some(id), &self.colors)
            .map(Message::Settings);
        frost::scoped(cosmic::theme::active(), self.settings_frosted(), view)
    }
}

/// Run the daemon UI. `runtime` must outlive the app: the zbus connection's tasks run on it.
pub fn run(daemon: Daemon, runtime: tokio::runtime::Runtime, host: Host) -> ExitCode {
    // Every user folder defaults under the home; without one they would land in the folder Signpost started in.
    if home_dir().is_none() {
        let e = "no home folder: HOME is relative, or unset and the user database has none";
        tracing::error!("{e}");
        eprintln!("signpost: {e}");
        runtime.block_on(daemon.fail());
        return ExitCode::from(1);
    }
    let Daemon {
        conn,
        readiness,
        events,
        backlog,
    } = daemon;
    let shutdown_conn = conn.clone();
    let flags = Flags {
        host,
        inbox: Inbox(Arc::new(Mutex::new(Some(events)))),
        conn,
        readiness: Mutex::new(Some(readiness)),
        backlog,
    };
    let settings = Settings::default()
        .no_main_window(true)
        .exit_on_close(false);
    let result = cosmic::app::run::<App>(settings, flags);
    // If init never ran, the dropped Readiness closes the state channel: pending calls get errors
    // (ApplicationIface::admit). Either way, finish in-flight replies before the runtime goes away.
    runtime.block_on(shutdown_conn.graceful_shutdown());
    drop(runtime);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!(error = %e, "signpost UI failed");
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod backlog_tests;
#[cfg(test)]
mod focus_tests;
#[cfg(test)]
mod frost_tests;
#[cfg(test)]
mod launch_tests;
#[cfg(test)]
mod menu_tests;
#[cfg(test)]
mod settings_tests;
#[cfg(test)]
mod world;

#[cfg(test)]
mod host_paths_tests;
#[cfg(test)]
mod tests;
