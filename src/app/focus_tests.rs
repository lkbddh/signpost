//! The picker's focus as the app drives it: the keys and presses it reads, the scrolling it asks for and
//! the native focus it clears.
//!
//! Building the app reads libcosmic's configuration and the app index, so these tests run again in a child
//! process whose home is a scratch directory.

use std::any::Any;
use std::time::Duration;

use cosmic::Application as _;
use cosmic::iced::event::Status;
use cosmic::iced::keyboard::key::{Code, Named, Physical};
use cosmic::iced::keyboard::{Key, Location, Modifiers};
use cosmic::iced::runtime::Action as RuntimeAction;
use cosmic::iced::runtime::platform_specific::Action as PlatformAction;
use cosmic::iced::runtime::platform_specific::wayland::Action as WaylandAction;
use cosmic::iced::runtime::platform_specific::wayland::activation::Action as ActivationAction;
use cosmic::iced::runtime::platform_specific::wayland::popup::SctkPopupSettings;
use cosmic::iced::{Point, Rectangle, Size, Vector, mouse, touch};
use cosmic::surface::Action as Surface;
use cosmic::surface::action::LiveSettings;
use futures_util::StreamExt;

use super::frost_tests::live_settings_of;
use super::world::World;
use super::*;
use crate::picker_view::tile_id;
use crate::profiles::Tile;
use crate::test_support::headless::{self, Drawn, Frames, NEW_POPUP, Probe};

const SCRATCH_HOME: &str = "SIGNPOST_APP_FOCUS_SCRATCH_HOME";
/// What the surface may grow to; the picker sizes itself within it.
const SURFACE: Size = Size::new(720.0, 560.0);
const SETTINGS_SURFACE: Size = Size::new(640.0, 600.0);
/// How long a task may take to send what is ready to be sent; a timer it waits on does not count.
const IDLE: Duration = Duration::from_millis(5);
/// Five rows of four tiles: more than the card shows at once, so the grid scrolls.
pub(super) const TALL: usize = 20;
const LAST_ROW_START: usize = 16;

/// Runs test `name` again with its home in a scratch directory, and says whether this is that run.
fn in_scratch_home(name: &str) -> bool {
    headless::in_scratch_home(SCRATCH_HOME, &format!("app::focus_tests::{name}"))
}

pub(super) fn plain_tile() -> Tile {
    Tile {
        app_id: "firefox.desktop".into(),
        app_name: "Firefox".into(),
        label: "Firefox".into(),
        icon: None,
        profile: None,
        flatpak: false,
        actions: Vec::new(),
    }
}

/// A world with one open picker of `tiles` tiles.
pub(super) fn world_with_picker(tiles: usize) -> (World, window::Id) {
    let mut world = World::new();
    let id = window::Id::unique();
    let picker = Picker::new("https://example.org/".into(), vec![plain_tile(); tiles]);
    world
        .app
        .pickers
        .insert(id, PickerState::new(picker, Vec::new()));
    (world, id)
}

pub(super) fn key_pressed(key: Named, modifiers: Modifiers) -> Event {
    let key = Key::Named(key);
    Event::Keyboard(keyboard::Event::KeyPressed {
        key: key.clone(),
        modified_key: key,
        physical_key: Physical::Code(Code::Enter),
        location: Location::Standard,
        modifiers,
        text: None,
        repeat: false,
    })
}

pub(super) fn press(key: Named) -> Event {
    key_pressed(key, Modifiers::empty())
}

pub(super) fn left_press() -> Event {
    Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
}

pub(super) fn right_press() -> Event {
    Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right))
}

/// The settings window as the frames last showed it.
struct Beside {
    id: window::Id,
    frames: Frames,
}

type PopupSettings = Box<dyn Fn(&mut App) -> SctkPopupSettings + Send + Sync>;
type PopupView =
    Box<dyn for<'a> Fn(&'a App) -> cosmic::Element<'a, cosmic::Action<Message>> + Send + Sync>;

/// What the runtime does with a popup request: the popup gets a surface, and widgets of its own.
struct Popup {
    id: window::Id,
    size: Size,
    frames: Frames,
    view: PopupView,
}

/// A popup request, as the app made it.
#[derive(Debug)]
pub(super) struct Request {
    pub(super) settings: SctkPopupSettings,
    pub(super) live: LiveSettings,
}

/// What the runtime was asked to do to a surface.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Seen {
    Opened(window::Id),
    Destroyed(window::Id),
    LayerDestroyed(window::Id),
    /// An activation token was asked for from this surface, `None` for no surface.
    TokenRequested(Option<window::Id>),
}

fn app_message(action: cosmic::Action<Message>) -> Message {
    match action {
        cosmic::Action::App(message) => message,
        other => panic!("a view sent more than a message of the app: {other:?}"),
    }
}

/// The picker of `id` as the frames last showed it, the settings window beside it once that is open, and
/// the popup of its actions menu once the app asked for one.
pub(super) struct Shown<'a> {
    pub(super) world: &'a mut World,
    pub(super) id: window::Id,
    frames: Frames,
    settings: Option<Beside>,
    popup: Option<Popup>,
    /// What the compositor answers a token request with; `None` where it never does.
    token: Option<String>,
    pub(super) requests: Vec<Request>,
    pub(super) seen: Vec<Seen>,
}

impl<'a> Shown<'a> {
    pub(super) fn new(world: &'a mut World, id: window::Id) -> Self {
        Self {
            world,
            id,
            frames: Frames::new(SURFACE),
            settings: None,
            popup: None,
            token: None,
            requests: Vec::new(),
            seen: Vec::new(),
        }
    }

    /// The compositor answers every token request at once, with a token.
    pub(super) fn with_tokens(mut self) -> Self {
        self.token = Some("a-token".to_owned());
        self
    }

    pub(super) fn with_settings(mut self) -> Self {
        let _runtime = self.world.runtime.enter();
        drop(self.world.app.open_settings(None));
        let id = self.world.app.settings.as_ref().expect("settings").id();
        self.settings = Some(Beside {
            id,
            frames: Frames::new(SETTINGS_SURFACE),
        });
        self
    }

    pub(super) fn picker(&self) -> &Picker {
        self.world.app.picker(self.id).expect("the picker")
    }

    /// The surface of the actions menu's popup, while it is open.
    pub(super) fn popup_id(&self) -> window::Id {
        self.popup.as_ref().expect("the app asked for no popup").id
    }

    pub(super) fn has_popup(&self) -> bool {
        self.popup.is_some()
    }

    /// The surface the popup was given.
    pub(super) fn popup_size(&self) -> Size {
        self.popup
            .as_ref()
            .expect("the app asked for no popup")
            .size
    }

    /// What the popup's widgets report about themselves.
    pub(super) fn popup_probe(&mut self) -> Probe {
        let popup = self.popup.as_mut().expect("the app asked for no popup");
        let view = (popup.view)(&self.world.app).map(app_message);
        popup.frames.probe(view)
    }

    /// The windows `only` names, or every window, with their frames and what they show now.
    fn windows(
        &mut self,
        only: Option<window::Id>,
    ) -> Vec<(window::Id, &mut Frames, Element<'_, Message>)> {
        let app = &self.world.app;
        let mut windows = Vec::new();
        if let Some(beside) = &mut self.settings {
            windows.push((beside.id, &mut beside.frames, app.view_window(beside.id)));
        }
        windows.push((self.id, &mut self.frames, app.picker_view(self.id)));
        if let Some(popup) = &mut self.popup {
            windows.push((
                popup.id,
                &mut popup.frames,
                (popup.view)(app).map(app_message),
            ));
        }
        windows.retain(|(id, ..)| only.is_none_or(|only| *id == only));
        windows
    }

    /// Does what the runtime does with `task`: its widget operations reach the widgets of every window,
    /// what it asks of surfaces is done and the messages it sends reach the app. A timer that has not
    /// fired is not waited for.
    pub(super) fn run(&mut self, task: Task<Message>) {
        self.run_in(None, task);
    }

    /// [`Shown::run`] for the window `only` names, as if no other window had any widgets.
    fn run_in(&mut self, only: Option<window::Id>, task: Task<Message>) {
        let Some(mut stream) = cosmic::iced::runtime::task::into_stream(task) else {
            return;
        };
        while let Ok(Some(action)) = self
            .world
            .runtime
            .block_on(async { tokio::time::timeout(IDLE, stream.next()).await })
        {
            match action {
                RuntimeAction::Widget(operation) => {
                    headless::operate_windows(self.windows(only), operation);
                }
                RuntimeAction::Output(cosmic::Action::App(message)) => self.update(message),
                RuntimeAction::Output(cosmic::Action::Surface(surface)) => self.surface(surface),
                RuntimeAction::PlatformSpecific(PlatformAction::Wayland(
                    WaylandAction::Activation(ActivationAction::RequestToken {
                        window,
                        channel,
                        ..
                    }),
                )) => self.request_token(window, channel),
                _ => {}
            }
        }
    }

    fn request_token(
        &mut self,
        window: Option<window::Id>,
        channel: cosmic::iced::futures::channel::oneshot::Sender<Option<String>>,
    ) {
        self.seen.push(Seen::TokenRequested(window));
        if let Some(token) = &self.token {
            drop(channel.send(Some(token.clone())));
        }
    }

    fn surface(&mut self, surface: Surface<Message>) {
        match surface {
            Surface::AppPopup(settings, live, view) => self.open_popup(settings, live, view),
            Surface::DestroyPopup(id) => self.destroy_popup(id),
            Surface::DestroyLayerShell(id) => self.seen.push(Seen::LayerDestroyed(id)),
            _ => {}
        }
    }

    fn open_popup(
        &mut self,
        settings: Arc<Box<dyn Any + Send + Sync>>,
        live: Arc<Box<dyn Any + Send + Sync>>,
        view: Option<Arc<Box<dyn Any + Send + Sync>>>,
    ) {
        let settings = *Arc::try_unwrap(settings)
            .expect("the action holds the only reference")
            .downcast::<PopupSettings>()
            .expect("a function of the app");
        let settings = settings(&mut self.world.app);
        let live = live_settings_of(live)(&self.world.app);
        let view = *Arc::try_unwrap(view.expect("the popup has a view"))
            .expect("the action holds the only reference")
            .downcast::<PopupView>()
            .expect("a view of the app");
        assert_eq!(settings.positioner.size, None, "a popup that sizes itself");
        let size = self.autosized(&view);
        self.seen.push(Seen::Opened(settings.id));
        self.popup = Some(Popup {
            id: settings.id,
            size,
            frames: Frames::new(size),
            view,
        });
        self.requests.push(Request { settings, live });
    }

    /// The surface the runtime gives a popup that sizes itself: the size its view asks for, when it is told
    /// to ask in the first frame, of a surface of [`NEW_POPUP`].
    fn autosized(&self, view: &PopupView) -> Size {
        Frames::new(NEW_POPUP)
            .requested_size(view(&self.world.app).map(app_message))
            .expect("a popup that sizes itself")
    }

    /// The popup goes, and the runtime tells the app it is gone, as the vendor does for every popup it
    /// destroys, a stale one included.
    fn destroy_popup(&mut self, id: window::Id) {
        self.seen.push(Seen::Destroyed(id));
        self.popup.take_if(|popup| popup.id == id);
        self.update(Message::SurfaceClosed(id));
    }

    /// The user tabs into the settings window: its next control takes the native focus.
    pub(super) fn tab_in_settings(&mut self) {
        let settings = self.settings.as_ref().map(|beside| beside.id);
        self.run_in(settings, cosmic::iced::widget::operation::focus_next());
    }

    /// The user tabs into the picker: its next control takes the native focus.
    pub(super) fn tab_in_picker(&mut self) {
        let picker = Some(self.id);
        self.run_in(picker, cosmic::iced::widget::operation::focus_next());
    }

    /// The user tabs into the popup: its next control takes the native focus.
    pub(super) fn tab_in_popup(&mut self) {
        let popup = Some(self.popup_id());
        self.run_in(popup, cosmic::iced::widget::operation::focus_next());
    }

    /// The app gets `event` on the picker's surface and the runtime does what it asks.
    pub(super) fn receive(&mut self, event: Event) {
        self.update(Message::Event(self.id, event));
    }

    /// The app gets `event` on the popup's surface and the runtime does what it asks.
    pub(super) fn receive_in_popup(&mut self, event: Event) {
        self.update(Message::Event(self.popup_id(), event));
    }

    /// The popup's widgets get `event` with the pointer at `at`, and send these; and whether they took it.
    pub(super) fn popup_answer(&mut self, event: Event, at: Point) -> (Vec<Message>, Status) {
        let popup = self.popup.as_mut().expect("the app asked for no popup");
        let view = (popup.view)(&self.world.app).map(app_message);
        let (messages, statuses) = popup.frames.send_with_statuses(view, &[event], at);
        (messages, statuses[0])
    }

    /// The user presses a key on the popup's surface: its widgets answer, the app gets what they send, and
    /// the key they left alone as an event.
    pub(super) fn press_in_popup(&mut self, event: Event) {
        let (messages, status) = self.popup_answer(event.clone(), Point::ORIGIN);
        for message in messages {
            self.update(message);
        }
        if forward_event(&event, status) {
            self.receive_in_popup(event);
        }
    }

    /// How far the popup's list of rows is scrolled, in pixels from its top.
    pub(super) fn menu_offset(&mut self) -> f32 {
        let popup = self.popup.as_mut().expect("the app asked for no popup");
        let view = (popup.view)(&self.world.app).map(app_message);
        let probe = popup.frames.probe(view);
        probe
            .scrollable_named(&MENU_SCROLL)
            .expect("the list of rows")
            .offset
            .y
    }

    /// The popup as it draws itself under `theme`.
    pub(super) fn popup_drawn(&mut self, theme: &cosmic::Theme) -> Drawn {
        let popup = self.popup.as_mut().expect("the app asked for no popup");
        let view = (popup.view)(&self.world.app).map(app_message);
        popup.frames.drawn(view, theme)
    }

    /// The bounds of the focus stops of the popup that have the native focus.
    pub(super) fn popup_focused(&mut self) -> Vec<Rectangle> {
        let popup = self.popup.as_mut().expect("the app asked for no popup");
        let view = (popup.view)(&self.world.app).map(app_message);
        popup.frames.probe(view).focused
    }

    /// Where tile `index` is on the picker's surface: where it was laid out, less what the grid scrolled.
    pub(super) fn tile_on_surface(&mut self, index: usize) -> Rectangle {
        let view = self.world.app.picker_view(self.id);
        let probe = self.frames.probe(view);
        let offset = probe
            .scrollable_named(&PICKER_SCROLL)
            .expect("the grid")
            .offset;
        let laid_out = probe.container_named(&tile_id(index)).expect("the tile");
        laid_out - Vector::new(offset.x, offset.y)
    }

    /// The picker's own widgets get `event` with the pointer on the picker's top left corner, and send these.
    pub(super) fn widgets_send(&mut self, event: Event) -> Vec<Message> {
        self.widgets_answer(event).0
    }

    /// [`Shown::widgets_send`], and whether the widgets took the event.
    pub(super) fn widgets_answer(&mut self, event: Event) -> (Vec<Message>, Status) {
        let view = self.world.app.picker_view(self.id);
        let (messages, statuses) = self
            .frames
            .send_with_statuses(view, &[event], Point::ORIGIN);
        (messages, statuses[0])
    }

    /// The user presses a key on the picker's surface: its widgets answer, the app gets what they send, and
    /// the key they left alone as an event.
    pub(super) fn press_in_picker(&mut self, event: Event) {
        let (messages, status) = self.widgets_answer(event.clone());
        for message in messages {
            self.update(message);
        }
        if forward_event(&event, status) {
            self.receive(event);
        }
    }

    /// The settings window's widgets get `event` and say whether they took it.
    pub(super) fn settings_answer(&mut self, event: Event) -> Status {
        let beside = self.settings.as_mut().expect("the settings are open");
        let view = self.world.app.view_window(beside.id);
        let (_, statuses) = beside
            .frames
            .send_with_statuses(view, &[event], Point::ORIGIN);
        statuses[0]
    }

    /// The model gets `input`, as the picker's widgets send it, and the runtime does what it asks.
    pub(super) fn input(&mut self, input: Input) {
        self.update(Message::Tile(self.id, input));
    }

    /// The app updates on `message`, which may start timers, and the runtime does what it asks.
    pub(super) fn update(&mut self, message: Message) {
        let _runtime = self.world.runtime.enter();
        let task = self.world.app.update(message);
        self.run(task);
    }

    /// Does what libcosmic does for a Tab press no widget took: the native focus moves on.
    pub(super) fn native_tab(&mut self) {
        let task = cosmic::iced::widget::operation::focus_next();
        self.run(task);
    }

    /// Does what libcosmic does for a Tab press (`shift` for Shift+Tab) on a surface whose widgets
    /// answered `status`: its keyboard navigation, while it is on, moves the native focus on unless a
    /// widget took the key (`keyboard_nav::subscription`, and its action in `app::cosmic`).
    fn libcosmic_tab(&mut self, status: Status, shift: bool) {
        if !self.world.app.core.keyboard_nav() || status == Status::Captured {
            return;
        }
        let walk = if shift {
            cosmic::iced::widget::operation::focus_previous()
        } else {
            cosmic::iced::widget::operation::focus_next()
        };
        self.run(walk);
    }

    /// The bounds of the focus stops of the picker that have the native focus.
    pub(super) fn picker_focused(&mut self) -> Vec<Rectangle> {
        let view = self.world.app.picker_view(self.id);
        self.frames.probe(view).focused
    }

    /// The bounds of the focus stops of the settings window, and of those that have the native focus.
    pub(super) fn settings_stops(&mut self) -> (Vec<Rectangle>, Vec<Rectangle>) {
        let beside = self.settings.as_mut().expect("the settings are open");
        let probe = beside.frames.probe(self.world.app.view_window(beside.id));
        (probe.stops, probe.focused)
    }

    /// How far the grid is scrolled, in pixels from its top.
    pub(super) fn grid_offset(&mut self) -> f32 {
        let view = self.world.app.picker_view(self.id);
        let probe = self.frames.probe(view);
        probe
            .scrollable_named(&PICKER_SCROLL)
            .expect("the grid")
            .offset
            .y
    }

    /// The user scrolls the grid with the wheel, to its top or to its bottom.
    pub(super) fn wheel_to_top(&mut self) {
        self.wheel_to(0.0);
    }

    pub(super) fn wheel_to_bottom(&mut self) -> f32 {
        self.wheel_to(1.0);
        self.grid_offset()
    }

    pub(super) fn wheel_to(&mut self, offset: f32) {
        let scroll = cosmic::iced::widget::scrollable::snap_to(
            PICKER_SCROLL.clone(),
            cosmic::iced::widget::scrollable::RelativeOffset {
                x: None,
                y: Some(offset),
            },
        );
        self.run(scroll);
    }
}

pub(super) fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

#[test]
fn a_press_or_a_touch_hides_the_focus_whichever_widget_takes_it() {
    if !in_scratch_home("a_press_or_a_touch_hides_the_focus_whichever_widget_takes_it") {
        return;
    }
    let touch = Event::Touch(touch::Event::FingerPressed {
        id: touch::Finger(1),
        position: Point::ORIGIN,
    });
    for (what, pressed) in [("a mouse press", left_press()), ("a touch", touch)] {
        let (mut world, id) = world_with_picker(3);
        let mut shown = Shown::new(&mut world, id);
        shown.receive(press(Named::ArrowRight));
        assert!(shown.picker().focus_visible, "{what}: the arrow showed it");
        shown.receive(pressed);
        assert!(!shown.picker().focus_visible, "{what}");
        assert_eq!(shown.picker().focus, 1, "{what}: and left it where it was");
    }
}

#[test]
fn the_app_reads_a_press_a_widget_took_and_not_the_pointer_moving() {
    use cosmic::iced::event::Status::{Captured, Ignored};
    assert!(forward_event(&left_press(), Captured));
    assert!(forward_event(
        &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)),
        Captured
    ));
    let moved = Event::Mouse(mouse::Event::CursorMoved {
        position: Point::ORIGIN,
    });
    assert!(!forward_event(&moved, Ignored));
}

#[test]
fn the_tab_key_is_one_the_picker_reads() {
    assert_eq!(
        key_from(&Key::Named(Named::Tab)),
        Some(crate::picker::Key::Tab)
    );
}

#[test]
fn an_arrow_reveals_the_focus_even_where_it_cannot_move_and_even_when_it_shows() {
    if !in_scratch_home(
        "an_arrow_reveals_the_focus_even_where_it_cannot_move_and_even_when_it_shows",
    ) {
        return;
    }
    let (mut world, id) = world_with_picker(TALL);
    let mut shown = Shown::new(&mut world, id);
    assert!(shown.wheel_to_bottom() > 0.0, "scrolled with the wheel");

    shown.receive(press(Named::ArrowLeft));
    assert_eq!(shown.picker().focus, 0, "it cannot go further");
    assert!(
        near(shown.grid_offset(), 0.0),
        "the grid shows the focus again"
    );

    shown.wheel_to_bottom();
    shown.receive(press(Named::ArrowLeft));
    assert!(
        near(shown.grid_offset(), 0.0),
        "the focus was shown already, and still out of view"
    );
}

#[test]
fn a_pointer_that_dismisses_the_menu_leaves_the_grid_where_the_user_scrolled_it() {
    if !in_scratch_home(
        "a_pointer_that_dismisses_the_menu_leaves_the_grid_where_the_user_scrolled_it",
    ) {
        return;
    }
    let (mut world, id) = world_with_picker(TALL);
    let mut shown = Shown::new(&mut world, id);
    shown.input(Input::OpenMenu(Some(TALL - 1)));
    shown.wheel_to_top();

    shown.receive(left_press());
    shown.input(Input::CloseMenu);
    assert_eq!(shown.picker().menu, None);
    assert!(near(shown.grid_offset(), 0.0), "no snap back to the tile");
}

#[test]
fn the_keyboard_closing_the_menu_shows_the_tile_it_belongs_to() {
    if !in_scratch_home("the_keyboard_closing_the_menu_shows_the_tile_it_belongs_to") {
        return;
    }
    let (mut world, id) = world_with_picker(TALL);
    let mut shown = Shown::new(&mut world, id);
    for _ in 0..4 {
        shown.receive(press(Named::ArrowDown));
    }
    assert_eq!(shown.picker().focus, LAST_ROW_START);
    let bottom = shown.wheel_to_bottom();
    shown.receive(press(Named::ContextMenu));
    shown.wheel_to_top();

    shown.receive(press(Named::Escape));
    assert_eq!(shown.picker().menu, None);
    assert!(
        near(shown.grid_offset(), bottom),
        "the grid scrolls back to the focus"
    );
}

#[test]
fn tab_hands_the_indicator_to_a_header_control_and_an_arrow_takes_it_back_for_one_launch() {
    if !in_scratch_home(
        "tab_hands_the_indicator_to_a_header_control_and_an_arrow_takes_it_back_for_one_launch",
    ) {
        return;
    }
    let (mut world, id) = world_with_picker(3);
    let mut shown = Shown::new(&mut world, id);
    shown.receive(press(Named::ArrowRight));
    assert!(shown.picker().focus_visible);

    shown.receive(press(Named::Tab));
    shown.native_tab();
    assert!(!shown.picker().focus_visible, "the tile's ring is gone");
    assert_eq!(shown.picker().focus, 1, "its tile is not forgotten");
    let [Message::Tile(_, Input::TogglePin)] = shown.widgets_send(press(Named::Enter))[..] else {
        panic!("Enter now acts on the header control that has the native focus");
    };

    shown.receive(press(Named::ArrowLeft));
    assert!(
        shown.picker().focus_visible,
        "an arrow takes the indicator back"
    );
    assert!(
        shown.widgets_send(press(Named::Enter)).is_empty(),
        "and the header control lost the native focus"
    );

    shown.receive(press(Named::Enter));
    assert_eq!(shown.world.app.attempt, 1, "one launch");
    assert!(!shown.picker().pinned, "and the header control did not act");
    let pending = shown
        .world
        .app
        .pickers
        .get(&id)
        .and_then(|state| state.pending.as_ref());
    assert_eq!(
        pending.map(|attempt| attempt.index),
        Some(0),
        "of the tile the arrow chose"
    );
}

#[test]
fn an_open_menu_captures_tab_and_shift_tab_on_its_own_surface() {
    if !in_scratch_home("an_open_menu_captures_tab_and_shift_tab_on_its_own_surface") {
        return;
    }
    let (mut world, id) = world_with_picker(3);
    let mut shown = Shown::new(&mut world, id);
    let (messages, status) = shown.widgets_answer(press(Named::Tab));
    assert_eq!(
        (messages.len(), status),
        (0, Status::Ignored),
        "with no menu the key is left to libcosmic"
    );

    shown.receive(press(Named::ContextMenu));
    for (modifiers, shift) in [(Modifiers::empty(), false), (Modifiers::SHIFT, true)] {
        let tab = key_pressed(Named::Tab, modifiers);
        let (messages, status) = shown.popup_answer(tab, Point::ORIGIN);
        assert_eq!(status, Status::Captured, "shift={shift}");
        let [Message::Tile(owner, input)] = &messages[..] else {
            panic!("the menu asks its picker to walk the rows: {messages:?}");
        };
        assert_eq!((*owner, input), (id, &Input::Tab { shift }));
    }
    let ctrl_tab = key_pressed(Named::Tab, Modifiers::CTRL);
    let (messages, status) = shown.popup_answer(ctrl_tab, Point::ORIGIN);
    assert_eq!(
        (messages.len(), status),
        (0, Status::Ignored),
        "Ctrl+Tab is no traversal"
    );
}

#[test]
fn tab_walks_the_open_menu_without_switching_libcosmics_navigation_off() {
    if !in_scratch_home("tab_walks_the_open_menu_without_switching_libcosmics_navigation_off") {
        return;
    }
    let (mut world, id) = world_with_picker(3);
    let mut shown = Shown::new(&mut world, id);
    shown.receive(press(Named::ContextMenu));
    assert!(
        shown.world.app.core.keyboard_nav(),
        "libcosmic's navigation stays on for every window"
    );

    shown.press_in_popup(press(Named::Tab));
    assert_eq!(shown.picker().menu_focus, 1, "one step, not two");
    shown.press_in_popup(key_pressed(Named::Tab, Modifiers::SHIFT));
    shown.press_in_popup(key_pressed(Named::Tab, Modifiers::SHIFT));
    assert_eq!(
        shown.picker().menu_focus,
        2,
        "Shift+Tab goes back, round the first row"
    );
    assert_eq!(shown.picker().menu, Some(0), "Tab leaves the menu open");
}

#[test]
fn a_menu_open_in_a_pinned_picker_leaves_tab_working_in_the_settings_window() {
    if !in_scratch_home("a_menu_open_in_a_pinned_picker_leaves_tab_working_in_the_settings_window")
    {
        return;
    }
    let (mut world, id) = world_with_picker(3);
    let mut shown = Shown::new(&mut world, id).with_settings();
    shown.input(Input::TogglePin);
    shown.receive(press(Named::ContextMenu));
    assert!(shown.picker().pinned && shown.picker().menu.is_some());

    let (stops, focused) = shown.settings_stops();
    assert!(stops.len() >= 2, "the settings have controls to walk");
    assert_eq!(focused, Vec::<Rectangle>::new());
    for (modifiers, shift, expected) in [
        (Modifiers::empty(), false, 0),
        (Modifiers::empty(), false, 1),
        (Modifiers::SHIFT, true, 0),
    ] {
        let status = shown.settings_answer(key_pressed(Named::Tab, modifiers));
        assert_eq!(status, Status::Ignored, "no menu on this surface");
        shown.libcosmic_tab(status, shift);
        let (_, focused) = shown.settings_stops();
        assert_eq!(
            focused,
            [stops[expected]],
            "shift={shift}: the settings' focus moves to their control {expected}"
        );
    }
}

#[test]
fn reclaiming_the_keyboard_clears_the_native_focus_of_the_picker_and_no_other_window() {
    if !in_scratch_home(
        "reclaiming_the_keyboard_clears_the_native_focus_of_the_picker_and_no_other_window",
    ) {
        return;
    }
    let (mut world, id) = world_with_picker(3);
    let mut shown = Shown::new(&mut world, id).with_settings();
    shown.input(Input::OpenMenu(Some(0)));
    shown.tab_in_settings();
    for _ in 0..3 {
        shown.tab_in_picker();
    }
    assert_eq!(
        shown.settings_stops().1.len(),
        1,
        "the settings have a focus"
    );
    assert_eq!(
        shown.picker_focused().len(),
        1,
        "the picker has one, on a header control"
    );

    shown.receive(press(Named::ArrowDown));
    assert!(
        shown.picker_focused().is_empty(),
        "the arrow took the picker's header controls out of the focus"
    );
    assert_eq!(
        shown.settings_stops().1.len(),
        1,
        "and left the settings window's focus alone"
    );
}

#[test]
fn a_menu_the_pointer_opens_takes_enter_from_the_header_control_that_kept_the_native_focus() {
    if !in_scratch_home(
        "a_menu_the_pointer_opens_takes_enter_from_the_header_control_that_kept_the_native_focus",
    ) {
        return;
    }
    let (mut world, id) = world_with_picker(3);
    let mut shown = Shown::new(&mut world, id);
    shown.receive(press(Named::Tab));
    shown.native_tab();
    let [Message::Tile(_, Input::TogglePin)] = shown.widgets_send(press(Named::Enter))[..] else {
        panic!("Tab left the native focus on the pin button");
    };

    shown.receive(right_press());
    shown.input(Input::OpenMenu(Some(1)));
    assert_eq!(shown.picker().menu, Some(1));
    assert!(!shown.picker().focus_visible, "the pointer shows no accent");

    shown.press_in_picker(press(Named::Enter));
    assert!(!shown.picker().pinned, "the pin button did not act");
    assert_eq!(
        shown.world.app.attempt, 1,
        "the menu's first row acted, once"
    );
    let pending = shown
        .world
        .app
        .pickers
        .get(&id)
        .and_then(|state| state.pending.as_ref());
    assert_eq!(
        pending.map(|attempt| attempt.index),
        Some(1),
        "on the tile the pointer opened it for"
    );
}
