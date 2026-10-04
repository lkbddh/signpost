//! The links the bus counted as waiting, followed through the app: each leaves the count when it is
//! presented or dropped, and the links the app queues itself are counted the same way.
//!
//! Building the app reads libcosmic's configuration and the app index, so each test runs again in a child
//! process whose home is a scratch directory.

use cosmic::Application as _;
use cosmic::iced::Size;
use cosmic::surface::Action as Surface;

use super::world::{Raised, World};
use super::*;
use crate::picker::{MAX_WAITING_LINKS, Target};
use crate::settings::Link;
use crate::test_support::headless;

const SCRATCH_HOME: &str = "SIGNPOST_APP_BACKLOG_SCRATCH_HOME";

/// Runs test `name` again with its home in a scratch directory, and says whether this is that run.
fn in_scratch_home(name: &str) -> bool {
    headless::in_scratch_home(SCRATCH_HOME, &format!("app::backlog_tests::{name}"))
}

fn open_call(uris: &[&str]) -> Message {
    Message::Bus(BusEvent::Open {
        uris: uris.iter().map(|uri| (*uri).to_owned()).collect(),
    })
}

fn activate_call(token: &str) -> Message {
    Message::Bus(BusEvent::Activate {
        token: Some(token.to_owned()),
    })
}

fn opened(id: window::Id) -> Message {
    Message::Event(
        id,
        Event::Window(window::Event::Opened {
            position: None,
            size: Size::new(640.0, 600.0),
        }),
    )
}

fn activating(window: window::Id, token: &str) -> Raised {
    Raised {
        focused: Vec::new(),
        activated: vec![(window, token.to_owned())],
    }
}

/// Closes picker `id` as the runtime sees it through: destroyed, then reported gone.
fn close_and_gone(app: &mut App, id: window::Id) {
    drop(app.close_picker(id));
    drop(app.update(Message::SurfaceClosed(id)));
}

/// The picker on screen; the app presents one at a time.
fn showing(app: &App) -> window::Id {
    let ids: Vec<window::Id> = app.pickers.keys().copied().collect();
    let [id] = ids.try_into().expect("one picker is showing");
    id
}

#[test]
fn the_links_of_an_open_call_leave_the_count_as_they_are_presented_or_dropped() {
    if !in_scratch_home(
        "the_links_of_an_open_call_leave_the_count_as_they_are_presented_or_dropped",
    ) {
        return;
    }
    let mut world = World::new();
    let app = &mut world.app;
    app.backlog.add(4);
    drop(app.update(open_call(&[
        "https://a/",
        "mailto:x@y",
        "https://b/",
        "https://c/",
    ])));
    assert_eq!(
        app.backlog.waiting(),
        2,
        "one link is presented and one dropped, two wait"
    );
    let first = showing(app);
    close_and_gone(app, first);
    assert_eq!(app.backlog.waiting(), 1, "b is presented");
    let second = showing(app);
    close_and_gone(app, second);
    assert_eq!(app.backlog.waiting(), 0, "c is presented");
}

#[test]
fn links_the_app_queues_itself_count_until_they_are_presented() {
    if !in_scratch_home("links_the_app_queues_itself_count_until_they_are_presented") {
        return;
    }
    let mut world = World::new();
    let app = &mut world.app;
    let test_link = || Message::Settings(settings::Message::OpenLink(settings::Link::Test));
    drop(app.update(test_link()));
    assert_eq!(
        app.backlog.waiting(),
        0,
        "nothing was showing: presented at once"
    );
    let source_link = Message::Settings(settings::Message::OpenLink(settings::Link::Source));
    drop(app.update(source_link));
    assert_eq!(app.backlog.waiting(), 1, "a link from Settings waits");
    let failure = LaunchFailure {
        uri: "https://example.org/".into(),
        failed: Target {
            app_id: "a".into(),
            profile_key: None,
            label: "A".into(),
        },
        message: "exited".into(),
    };
    drop(app.update(Message::EarlyFailure {
        source: window::Id::unique(),
        attempt: 1,
        failure,
    }));
    assert_eq!(
        app.backlog.waiting(),
        2,
        "so does a link whose picker closed on a failure"
    );
    let first = showing(app);
    close_and_gone(app, first);
    assert_eq!(app.backlog.waiting(), 1);
    let second = showing(app);
    close_and_gone(app, second);
    assert_eq!(app.backlog.waiting(), 0);
}

#[test]
fn shutting_down_frees_the_links_still_waiting() {
    if !in_scratch_home("shutting_down_frees_the_links_still_waiting") {
        return;
    }
    let mut world = World::new();
    let app = &mut world.app;
    app.backlog.add(3);
    drop(app.update(open_call(&["https://a/", "https://b/", "https://c/"])));
    assert_eq!(app.backlog.waiting(), 2);
    drop(app.update(Message::Shutdown));
    assert_eq!(app.backlog.waiting(), 0);
}

#[test]
fn settings_links_at_the_waiting_limit_are_refused_with_a_toast_and_nothing_is_queued() {
    if !in_scratch_home(
        "settings_links_at_the_waiting_limit_are_refused_with_a_toast_and_nothing_is_queued",
    ) {
        return;
    }
    let mut world = World::new();
    drop(world.app.open_settings(None));
    let app = &mut world.app;
    app.backlog.add(MAX_WAITING_LINKS);
    for link in [Link::Test, Link::Source, Link::Issues, Link::License] {
        drop(app.update(Message::Settings(settings::Message::OpenLink(link))));
        assert!(app.pickers.is_empty(), "{link:?} was presented");
        assert_eq!(app.queue.waiting(), 0, "{link:?} was queued");
        assert_eq!(
            app.backlog.waiting(),
            MAX_WAITING_LINKS,
            "{link:?} was counted"
        );
        let toasts = format!(
            "{:?}",
            app.settings.as_ref().expect("a settings window").toasts
        );
        assert!(toasts.contains("Too many links are waiting"), "{toasts}");
    }
}

#[test]
fn a_settings_link_just_below_the_limit_is_queued() {
    if !in_scratch_home("a_settings_link_just_below_the_limit_is_queued") {
        return;
    }
    let mut world = World::new();
    let app = &mut world.app;
    app.backlog.add(MAX_WAITING_LINKS - 1);
    drop(app.update(Message::Settings(settings::Message::OpenLink(Link::Test))));
    assert_eq!(app.pickers.len(), 1, "presented at once");
    assert_eq!(
        app.backlog.waiting(),
        MAX_WAITING_LINKS - 1,
        "presented, so not waiting"
    );
}

#[test]
fn an_activate_opens_one_settings_window_raises_it_and_counts_no_link() {
    if !in_scratch_home("an_activate_opens_one_settings_window_raises_it_and_counts_no_link") {
        return;
    }
    let mut world = World::new();
    let opening = world.app.update(activate_call("tok"));
    let [Surface::AppWindow(..)] = world.surface_actions(opening)[..] else {
        panic!("the first activate opens one window");
    };
    let window = world.app.settings.as_ref().expect("settings are open").id();
    drop(world.app.update(opened(window)));
    let raising = world.app.update(activate_call("again"));
    assert_eq!(
        world.raised(raising),
        activating(window, "again"),
        "the second activate asks the compositor to raise the open window"
    );
    let without_token = world
        .app
        .update(Message::Bus(BusEvent::Activate { token: None }));
    assert_eq!(
        world.raised(without_token),
        Raised {
            focused: vec![window],
            activated: Vec::new(),
        },
        "with no token the window is only focused"
    );
    assert_eq!(
        world.app.settings.as_ref().map(settings::Window::id),
        Some(window),
        "no second window"
    );
    assert_eq!(world.app.backlog.waiting(), 0);
}

#[test]
fn an_activate_raises_a_new_settings_window_once_it_exists() {
    if !in_scratch_home("an_activate_raises_a_new_settings_window_once_it_exists") {
        return;
    }
    let mut world = World::new();
    let opening = world.app.update(activate_call("tok"));
    assert_eq!(
        world.raised(opening),
        Raised::default(),
        "there is no surface to raise yet"
    );
    let window = world.app.settings.as_ref().expect("settings are open").id();
    let elsewhere = world.app.update(opened(window::Id::unique()));
    assert_eq!(
        world.raised(elsewhere),
        Raised::default(),
        "another window opening is not the settings window"
    );
    let exists = world.app.update(opened(window));
    assert_eq!(world.raised(exists), activating(window, "tok"));
    let again = world.app.update(opened(window));
    assert_eq!(
        world.raised(again),
        Raised::default(),
        "a token is used once"
    );
}

#[test]
fn a_token_that_arrives_while_settings_opens_waits_for_the_window() {
    if !in_scratch_home("a_token_that_arrives_while_settings_opens_waits_for_the_window") {
        return;
    }
    let mut world = World::new();
    drop(
        world
            .app
            .update(Message::Bus(BusEvent::Activate { token: None })),
    );
    let window = world.app.settings.as_ref().expect("settings are open").id();
    let waiting = world.app.update(activate_call("tok"));
    assert_eq!(
        world.raised(waiting),
        Raised::default(),
        "the runtime ignores a window it does not track yet"
    );
    let exists = world.app.update(opened(window));
    assert_eq!(world.raised(exists), activating(window, "tok"));
    let again = world.app.update(opened(window));
    assert_eq!(
        world.raised(again),
        Raised::default(),
        "a token is used once"
    );
}

#[test]
fn the_latest_token_that_arrives_while_settings_opens_is_applied() {
    if !in_scratch_home("the_latest_token_that_arrives_while_settings_opens_is_applied") {
        return;
    }
    let mut world = World::new();
    drop(world.app.update(activate_call("a")));
    let window = world.app.settings.as_ref().expect("settings are open").id();
    let waiting = world.app.update(activate_call("b"));
    assert_eq!(world.raised(waiting), Raised::default());
    let exists = world.app.update(opened(window));
    assert_eq!(world.raised(exists), activating(window, "b"));
    let again = world.app.update(opened(window));
    assert_eq!(
        world.raised(again),
        Raised::default(),
        "a token is used once"
    );
}

#[test]
fn an_activate_without_a_token_while_settings_opens_keeps_the_token_waiting() {
    if !in_scratch_home("an_activate_without_a_token_while_settings_opens_keeps_the_token_waiting")
    {
        return;
    }
    let mut world = World::new();
    drop(world.app.update(activate_call("tok")));
    let window = world.app.settings.as_ref().expect("settings are open").id();
    let waiting = world
        .app
        .update(Message::Bus(BusEvent::Activate { token: None }));
    assert_eq!(world.raised(waiting), Raised::default());
    let exists = world.app.update(opened(window));
    assert_eq!(world.raised(exists), activating(window, "tok"));
}

#[test]
fn a_link_the_bus_brings_twice_at_once_opens_one_picker() {
    if !in_scratch_home("a_link_the_bus_brings_twice_at_once_opens_one_picker") {
        return;
    }
    let mut world = World::new();
    let app = &mut world.app;
    for _ in 0..2 {
        app.backlog.add(1);
        drop(app.update(open_call(&["https://example.org/twice"])));
    }
    assert_eq!(app.pickers.len(), 1, "one picker shows");
    assert_eq!(app.queue.waiting(), 0, "nothing waits behind it");
    assert_eq!(app.backlog.waiting(), 0, "the repeat gave its room back");
}

/// Whether `actions` open a picker's layer.
fn opens_a_picker(actions: &[Surface<Message>]) -> bool {
    actions
        .iter()
        .any(|action| matches!(action, Surface::AppLayerShell(..)))
}

/// A picker showing the first of two links, with the second waiting.
fn two_links(world: &mut World) -> window::Id {
    world.app.backlog.add(2);
    for uri in ["https://example.org/first", "https://example.org/second"] {
        drop(world.app.update(open_call(&[uri])));
    }
    *world
        .app
        .pickers
        .keys()
        .next()
        .expect("the first picker shows")
}

#[test]
fn the_next_picker_shows_only_once_the_last_is_gone() {
    if !in_scratch_home("the_next_picker_shows_only_once_the_last_is_gone") {
        return;
    }
    let mut world = World::new();
    let first = two_links(&mut world);
    let closing = world.app.close_picker(first);
    let actions = world.surface_actions(closing);
    assert!(
        actions
            .iter()
            .any(|action| matches!(action, Surface::DestroyLayerShell(id) if *id == first)),
        "the first picker goes: {actions:?}"
    );
    assert!(
        !opens_a_picker(&actions),
        "no picker opens while the first is still on screen: {actions:?}"
    );
    let gone = world.app.update(Message::SurfaceClosed(first));
    assert!(
        opens_a_picker(&world.surface_actions(gone)),
        "the second picker opens once the first is gone"
    );
    assert_eq!(world.app.pickers.len(), 1);
}

#[test]
fn a_link_that_comes_while_the_last_picker_closes_waits_for_it_to_be_gone() {
    if !in_scratch_home("a_link_that_comes_while_the_last_picker_closes_waits_for_it_to_be_gone") {
        return;
    }
    let mut world = World::new();
    world.app.backlog.add(2);
    drop(world.app.update(open_call(&["https://example.org/first"])));
    let first = showing(&world.app);
    let closing = world.app.close_picker(first);
    drop(world.surface_actions(closing));
    let arrival = world.app.update(open_call(&["https://example.org/next"]));
    assert!(
        !opens_a_picker(&world.surface_actions(arrival)),
        "no picker opens while the first is still on screen"
    );
    let gone = world.app.update(Message::SurfaceClosed(first));
    assert!(
        opens_a_picker(&world.surface_actions(gone)),
        "the link shows once the first is gone"
    );
    assert_eq!(world.app.backlog.waiting(), 0);
}

#[test]
fn a_picker_the_compositor_closed_hands_over_at_once() {
    if !in_scratch_home("a_picker_the_compositor_closed_hands_over_at_once") {
        return;
    }
    let mut world = World::new();
    let first = two_links(&mut world);
    let gone = world.app.update(Message::SurfaceClosed(first));
    assert!(
        opens_a_picker(&world.surface_actions(gone)),
        "the second picker opens: the first's surface is already gone"
    );
    let ids: Vec<window::Id> = world.app.pickers.keys().copied().collect();
    assert_eq!(ids.len(), 1);
    assert_ne!(ids[0], first);
}
