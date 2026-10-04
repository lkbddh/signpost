//! The app's frosted-glass wiring, driven as libcosmic would drive it, with no compositor.
//!
//! Building the app reads libcosmic's configuration and the app index, so each test runs again in a child
//! process whose home is a scratch directory.

use std::hash::Hasher as _;
use std::sync::Arc;
use std::time::Duration;

use cosmic::Application as _;
use cosmic::core::Auto;
use cosmic::iced::advanced::subscription::{Event as SubscriptionEvent, Hasher, into_recipes};
use cosmic::iced::{Point, Size};
use cosmic::surface::Action as Surface;
use cosmic::surface::action::LiveSettings;
use cosmic::theme;
use futures_util::StreamExt;

use super::world::World;
use super::*;
use crate::test_support::frosted::{fill, settings as frosted_settings};
use crate::test_support::headless::{self, BACKDROP, Frames, is_rounding_apart, over};

const SCRATCH_HOME: &str = "SIGNPOST_APP_SCRATCH_HOME";
const LINK: &str = "https://example.org/";
/// The most a subscription's first message may take; they send theirs at once.
const SUBSCRIPTION_WAIT: Duration = Duration::from_secs(5);
const PICKER_SURFACE: Size = Size::new(720.0, 560.0);
const SETTINGS_SURFACE: Size = Size::new(360.0, 400.0);
/// How far inside a surface's edge its background is sampled, clear of every widget.
const EDGE_INSET: f32 = 3.0;

pub(super) type Live = Box<dyn Fn(&App) -> LiveSettings + Send + Sync>;

/// Runs test `name` again with its home in a scratch directory, and says whether this is that run.
fn in_scratch_home(name: &str) -> bool {
    headless::in_scratch_home(SCRATCH_HOME, &format!("app::frost_tests::{name}"))
}

/// The settings every combination of the user's three frosted-glass options gives, as (windows, system
/// interface, maximized apps).
fn every_setting() -> impl Iterator<Item = (bool, bool, bool)> {
    (0..8_u8).map(|bits| (bits & 1 != 0, bits & 2 != 0, bits & 4 != 0))
}

impl World {
    /// Opens a picker, and gives its surface and the live settings it asked libcosmic to keep asking.
    fn open_picker(&mut self) -> (window::Id, Live) {
        let task = self.app.open_picker(QueuedLink::new(LINK.to_owned()));
        let [opening] = self
            .surface_actions(task)
            .try_into()
            .expect("one surface action");
        let Surface::AppLayerShell(_, live, _) = opening else {
            panic!("the picker is a layer surface: {opening:?}");
        };
        let id = *self.app.pickers.keys().next().expect("the picker is open");
        (id, live_settings_of(live))
    }

    fn open_settings(&mut self) -> (window::Id, Live) {
        let task = self.app.open_settings(None);
        let [opening] = self
            .surface_actions(task)
            .try_into()
            .expect("one surface action");
        let Surface::AppWindow(_, _, live, _) = opening else {
            panic!("the settings are a window: {opening:?}");
        };
        let id = self.app.settings.as_ref().expect("settings are open").id();
        (id, live_settings_of(live))
    }

    /// The user's frosted-glass options change, as libcosmic reports a new system theme.
    fn choose(&mut self, (windows, system_interface, maximized_apps): (bool, bool, bool)) {
        let theme = frosted_settings(windows, system_interface, maximized_apps);
        drop(self.app.system_theme_update(&[], &theme));
    }

    fn compositor_announces_blur(&mut self) {
        let event = Event::PlatformSpecific(PlatformSpecific::Wayland(WaylandEvent::BlurEnabled));
        drop(self.app.update(Message::Event(window::Id::RESERVED, event)));
    }

    fn maximize_settings(&mut self, maximized: bool) {
        let message = Message::Settings(settings::Message::Maximized(maximized));
        drop(self.app.update(message));
    }

    /// The identities of the subscriptions the app runs now.
    fn subscriptions(&self) -> Vec<u64> {
        into_recipes(self.app.subscription())
            .iter()
            .map(|recipe| {
                let mut hasher = Hasher::default();
                recipe.hash(&mut hasher);
                hasher.finish()
            })
            .collect()
    }

    /// What the subscriptions that did not run in `known` would send, as they start.
    fn sent_since(&self, known: &[u64]) -> Vec<Message> {
        into_recipes(self.app.subscription())
            .into_iter()
            .filter(|recipe| {
                let mut hasher = Hasher::default();
                recipe.hash(&mut hasher);
                !known.contains(&hasher.finish())
            })
            .filter_map(|recipe| {
                let mut sent =
                    recipe.stream(futures_util::stream::empty::<SubscriptionEvent>().boxed());
                self.runtime
                    .block_on(async { tokio::time::timeout(SUBSCRIPTION_WAIT, sent.next()).await })
                    .ok()
                    .flatten()
            })
            .collect()
    }

    /// The surfaces libcosmic is asked to read the live settings of again after `message`.
    fn synced_by(&mut self, message: Message) -> Vec<window::Id> {
        let task = self.app.update(message);
        self.surface_actions(task)
            .into_iter()
            .filter_map(|action| match action {
                Surface::SyncLiveSettings(id) => Some(id),
                _ => None,
            })
            .collect()
    }
}

pub(super) fn live_settings_of(live: Arc<Box<dyn std::any::Any + Send + Sync>>) -> Live {
    *Arc::try_unwrap(live)
        .expect("the action holds the only reference")
        .downcast::<Live>()
        .expect("a function of the app")
}

fn is_sync(message: &Message) -> bool {
    matches!(message, Message::SyncSurfaces)
}

#[test]
fn the_app_leaves_the_blur_of_layer_surfaces_to_itself() {
    if !in_scratch_home("the_app_leaves_the_blur_of_layer_surfaces_to_itself") {
        return;
    }
    let world = World::new();
    let automatic = world.app.core().auto_blur();
    assert!(automatic.contains(Auto::Window), "windows keep libcosmic's");
    assert!(automatic.contains(Auto::Popup), "popups keep libcosmic's");
    assert!(
        !automatic.contains(Auto::System),
        "libcosmic's updates would overwrite the picker's blur"
    );
}

#[test]
fn the_compositors_blur_announcement_reaches_the_app() {
    let announcement =
        Event::PlatformSpecific(PlatformSpecific::Wayland(WaylandEvent::BlurEnabled));
    assert!(forward_event(&announcement, event::Status::Ignored));
}

#[test]
fn the_picker_asks_for_blur_by_the_system_interface_setting() {
    if !in_scratch_home("the_picker_asks_for_blur_by_the_system_interface_setting") {
        return;
    }
    let mut world = World::new();
    let (_, live) = world.open_picker();
    for supported in [false, true] {
        if supported {
            world.compositor_announces_blur();
        }
        for choice @ (_, system_interface, _) in every_setting() {
            world.choose(choice);
            assert_eq!(
                live(&world.app).blur,
                Some(supported && system_interface),
                "supported={supported}, {choice:?}"
            );
        }
    }
}

#[test]
fn the_settings_window_asks_for_blur_by_the_windows_setting_and_its_size() {
    if !in_scratch_home("the_settings_window_asks_for_blur_by_the_windows_setting_and_its_size") {
        return;
    }
    let mut world = World::new();
    let (_, live) = world.open_settings();
    for supported in [false, true] {
        if supported {
            world.compositor_announces_blur();
        }
        for choice @ (windows, _, maximized_apps) in every_setting() {
            world.choose(choice);
            for maximized in [false, true] {
                world.maximize_settings(maximized);
                let vetoed = maximized && !maximized_apps;
                let expected = supported && windows && !vetoed;
                assert_eq!(
                    live(&world.app).blur,
                    Some(expected),
                    "supported={supported}, {choice:?}, maximized={maximized}"
                );
            }
        }
    }
}

#[test]
fn turning_system_frosting_off_while_a_picker_is_open_has_it_synced_and_its_blur_disabled() {
    if !in_scratch_home(
        "turning_system_frosting_off_while_a_picker_is_open_has_it_synced_and_its_blur_disabled",
    ) {
        return;
    }
    let mut world = World::new();
    world.compositor_announces_blur();
    world.choose((false, true, false));
    let (id, live) = world.open_picker();
    assert_eq!(live(&world.app).blur, Some(true));

    let before = world.subscriptions();
    world.choose((false, false, false));
    let sent = world.sent_since(&before);
    assert!(
        matches!(sent.as_slice(), [message] if is_sync(message)),
        "{sent:?}"
    );
    assert_eq!(world.synced_by(Message::SyncSurfaces), [id]);
    assert_eq!(live(&world.app).blur, Some(false), "an explicit disable");

    let before = world.subscriptions();
    world.choose((false, true, false));
    let sent = world.sent_since(&before);
    assert!(
        matches!(sent.as_slice(), [message] if is_sync(message)),
        "{sent:?}"
    );
    assert_eq!(live(&world.app).blur, Some(true), "an explicit enable");
}

#[test]
fn maximizing_and_restoring_the_settings_window_has_it_synced() {
    if !in_scratch_home("maximizing_and_restoring_the_settings_window_has_it_synced") {
        return;
    }
    let mut world = World::new();
    world.compositor_announces_blur();
    world.choose((true, false, false));
    let (id, live) = world.open_settings();
    for (maximized, blur) in [(true, false), (false, true)] {
        let before = world.subscriptions();
        world.maximize_settings(maximized);
        let sent = world.sent_since(&before);
        assert!(
            matches!(sent.as_slice(), [message] if is_sync(message)),
            "maximized={maximized}: {sent:?}"
        );
        assert_eq!(world.synced_by(Message::SyncSurfaces), [id]);
        assert_eq!(live(&world.app).blur, Some(blur), "maximized={maximized}");
    }
}

#[test]
fn a_blur_announcement_after_a_picker_opened_has_it_synced() {
    if !in_scratch_home("a_blur_announcement_after_a_picker_opened_has_it_synced") {
        return;
    }
    let mut world = World::new();
    world.choose((false, true, false));
    let (_, live) = world.open_picker();
    assert_eq!(live(&world.app).blur, Some(false), "nothing can blur yet");

    let before = world.subscriptions();
    world.compositor_announces_blur();
    let sent = world.sent_since(&before);
    assert!(
        matches!(sent.as_slice(), [message] if is_sync(message)),
        "{sent:?}"
    );
    assert_eq!(live(&world.app).blur, Some(true));
}

#[test]
fn a_surface_opening_or_closing_has_the_open_surfaces_synced() {
    if !in_scratch_home("a_surface_opening_or_closing_has_the_open_surfaces_synced") {
        return;
    }
    let mut world = World::new();
    for opening in [true, false] {
        let before = world.subscriptions();
        if opening {
            drop(world.open_picker());
        } else {
            let id = *world.app.pickers.keys().next().expect("the picker is open");
            drop(world.app.update(Message::SurfaceClosed(id)));
        }
        let sent = world.sent_since(&before);
        assert!(
            matches!(sent.as_slice(), [message] if is_sync(message)),
            "opening={opening}: {sent:?}"
        );
    }
}

#[test]
fn a_change_no_open_surface_follows_syncs_nothing() {
    if !in_scratch_home("a_change_no_open_surface_follows_syncs_nothing") {
        return;
    }
    let mut world = World::new();
    world.compositor_announces_blur();
    world.choose((true, false, false));
    drop(world.open_picker());
    let before = world.subscriptions();
    world.choose((false, false, true));
    assert!(
        world.sent_since(&before).is_empty(),
        "only a picker is open, and it follows the system interface setting"
    );
}

#[test]
fn a_sync_reaches_every_open_surface() {
    if !in_scratch_home("a_sync_reaches_every_open_surface") {
        return;
    }
    let mut world = World::new();
    let (picker, _) = world.open_picker();
    let (settings, _) = world.open_settings();
    let mut expected = [picker, settings];
    expected.sort();
    assert_eq!(world.synced_by(Message::SyncSurfaces), expected);
}

/// The theme the app draws with, translucent as `frosted` says.
fn active_theme(frosted: bool) -> cosmic::Theme {
    let mut theme = cosmic::theme::active();
    theme.transparent = frosted;
    theme
}

#[test]
fn the_picker_draws_by_the_system_interface_setting_whatever_the_app_wide_theme_is() {
    if !in_scratch_home(
        "the_picker_draws_by_the_system_interface_setting_whatever_the_app_wide_theme_is",
    ) {
        return;
    }
    let mut world = World::new();
    world.compositor_announces_blur();
    let (id, _) = world.open_picker();
    let mut frames = Frames::new(PICKER_SURFACE);
    for choice @ (_, system_interface, _) in every_setting() {
        world.choose(choice);
        let card = frames
            .probe(world.app.picker_view(id))
            .container_named(&PICKER_AUTOSIZE)
            .expect("the card");
        let at = Point::new(card.x + EDGE_INSET, card.y + card.height / 2.0);
        let drawn = frames.drawn(world.app.picker_view(id), &cosmic::Theme::dark());
        let expected = over(
            fill(
                &active_theme(system_interface),
                &theme::Container::Background,
            ),
            BACKDROP,
        );
        let seen = drawn.at(at);
        assert!(
            is_rounding_apart(seen, expected),
            "{choice:?}: drawn {seen:?}, a picker frosted={system_interface} gives {expected:?}"
        );
    }
}

#[test]
fn the_settings_window_draws_by_the_windows_setting_and_its_size() {
    if !in_scratch_home("the_settings_window_draws_by_the_windows_setting_and_its_size") {
        return;
    }
    let mut world = World::new();
    world.compositor_announces_blur();
    let (id, _) = world.open_settings();
    let mut frames = Frames::new(SETTINGS_SURFACE);
    let gutter = Point::new(EDGE_INSET, SETTINGS_SURFACE.height / 2.0);
    for choice @ (windows, _, maximized_apps) in every_setting() {
        world.choose(choice);
        for maximized in [false, true] {
            world.maximize_settings(maximized);
            let drawn = frames.drawn(world.app.view_window(id), &cosmic::Theme::dark());
            let vetoed = maximized && !maximized_apps;
            let frosted = windows && !vetoed;
            let expected = over(
                fill(&active_theme(frosted), &theme::Container::WindowBackground),
                BACKDROP,
            );
            let seen = drawn.at(gutter);
            assert!(
                is_rounding_apart(seen, expected),
                "{choice:?}, maximized={maximized}: drawn {seen:?}, a window frosted={frosted} gives {expected:?}"
            );
        }
    }
}
