use std::cell::RefCell;
use std::path::PathBuf;

use cosmic::iced::keyboard;
use futures_util::StreamExt;

use super::*;
use crate::app::valid_link_uris;
use crate::colors::override_key;
use crate::index::Registry;
use crate::picker::{LinkQueue, QueuedLink};
use crate::profiles::Rgb;
use crate::settings::HandlerView;
use crate::settings::drawer;
use crate::settings::tests::plain;

mod layout;

const DARK: bool = true;
/// Signpost run on the host itself, as these tests have it.
static NATIVE: Host = Host::Native;
const CHROME_FOR_WEB: &str = "[Default Applications]\nx-scheme-handler/http=google-chrome.desktop\nx-scheme-handler/https=google-chrome.desktop\n";
const CHROME_FOR_HTTPS: &str =
    "[Default Applications]\nx-scheme-handler/https=google-chrome.desktop\n";

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// A config directory of `mimeapps` lists (highest precedence first; `mimeapps.list` is where setup
/// writes) and a state directory, all temporary, next to the fixture app index.
struct World {
    dir: tempfile::TempDir,
    registry: Registry,
    ops: Operations,
    home: PathBuf,
    config_home: PathBuf,
    state: PathBuf,
    lists: Vec<PathBuf>,
    color_store: ColorStore,
    colors: RefCell<Overrides>,
}

impl World {
    fn new(lists: &[(&str, &str)]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        std::fs::create_dir_all(&config).unwrap();
        for (name, text) in lists {
            std::fs::write(config.join(name), text).unwrap();
        }
        let home = fixtures().join("homes/a");
        let lists: Vec<PathBuf> = lists.iter().map(|(name, _)| config.join(name)).collect();
        Self {
            registry: Registry::load(
                &[
                    fixtures().join("applications-user"),
                    fixtures().join("applications-system"),
                ],
                &["en".into()],
            ),
            ops: Operations::default(),
            config_home: home.join(".config"),
            home,
            state: dir.path().join("state"),
            lists,
            color_store: ColorStore::at(dir.path().join("colors")).unwrap(),
            colors: RefCell::default(),
            dir,
        }
    }

    fn list(&self, name: &str) -> PathBuf {
        self.dir.path().join("config").join(name)
    }

    fn sources(&self) -> Sources<'_> {
        Sources {
            registry: &self.registry,
            ops: &self.ops,
            env: Env {
                home: &self.home,
                config_home: &self.config_home,
                view: &crate::index::Native,
            },
            state_dir: &self.state,
            lists: self.lists.clone(),
            user_list: Some(self.list("mimeapps.list")),
            host: &NATIVE,
            color_store: Some(&self.color_store),
        }
    }

    fn open(&self) -> Window {
        Window::open(window::Id::unique(), DARK, &self.sources())
    }

    fn files(&self) -> Files {
        Files {
            user_list: Some(self.list("mimeapps.list")),
            lists: self.lists.clone(),
            state_dir: self.state.clone(),
        }
    }

    /// What the operations do to the files, as the window's own run would.
    fn set_default(&self) -> Result<(), OpError> {
        execute(Op::Set, &self.files())
    }

    fn restore(&self) -> Result<(), OpError> {
        execute(Op::Restore, &self.files())
    }

    /// What the app does with a settings message: a completion is settled before any window sees it.
    fn deliver(&self, window: &mut Window, message: Message) -> Task<Message> {
        let Some(message) = self.ops.settle(message) else {
            return Task::none();
        };
        window.update(message, &self.sources(), &mut self.colors.borrow_mut())
    }

    fn send(&self, window: &mut Window, message: Message) {
        drop(self.deliver(window, message));
    }

    /// The end of the operation in flight, as its task would report it.
    fn completion(&self, result: Result<(), OpError>) -> Message {
        let Running { id, op } = self.ops.running.get().expect("an operation is in flight");
        Message::Done { id, op, result }
    }
}

const CHROME_FIRST: [&str; 6] = [
    "google-chrome",
    "org.example.DbusOnly",
    "env-browser",
    "escaped",
    "firefox",
    "term",
];
/// Chrome stops being the default, so it takes its place among the apps that declare the scheme.
const BY_NAME: [&str; 6] = [
    "org.example.DbusOnly",
    "env-browser",
    "escaped",
    "firefox",
    "google-chrome",
    "term",
];

/// Whether a toast shows `text`. The toasts are read through their debug text, where Fluent's isolation marks
/// around a name are escaped.
fn toast_shown(window: &Window, text: &str) -> bool {
    format!("{:?}", window.toasts)
        .replace(r"\u{2068}", "")
        .replace(r"\u{2069}", "")
        .contains(text)
}

fn app_ids(window: &Window) -> Vec<&str> {
    window.inventory.iter().map(|app| app.id.as_str()).collect()
}

/// The messages `task` sends. The tests run on a paused clock, so a toast's timer fires at once.
async fn sent(task: Task<Message>) -> Vec<Message> {
    let Some(stream) = cosmic::iced::runtime::task::into_stream(task) else {
        return Vec::new();
    };
    stream
        .filter_map(|action| async move {
            match action {
                cosmic::iced::runtime::Action::Output(cosmic::Action::App(message)) => {
                    Some(message)
                }
                _ => None,
            }
        })
        .collect()
        .await
}

#[test]
fn a_new_window_starts_on_the_main_page_and_every_page_leads_back_to_it() {
    let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    let mut window = world.open();
    assert_eq!(window.page, Page::Main);
    for page in [Page::Shortcuts, Page::About, Page::Main] {
        world.send(&mut window, Message::Go(page));
        assert_eq!(window.page, page);
    }
}

#[test]
fn closing_drops_only_the_settings_window_and_reopening_starts_on_the_main_page() {
    let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    let id = window::Id::unique();
    let mut slot = Some(Window::open(id, DARK, &world.sources()));
    world.send(slot.as_mut().unwrap(), Message::Go(Page::About));

    assert!(
        !Window::closed(&mut slot, window::Id::unique()),
        "a picker closing is not the settings window closing"
    );
    assert_eq!(slot.as_ref().map(|w| w.page), Some(Page::About));

    assert!(Window::closed(&mut slot, id));
    assert!(slot.is_none());
    assert!(
        !Window::closed(&mut slot, id),
        "a second close report finds nothing"
    );

    let reopened = Window::open(window::Id::unique(), DARK, &world.sources());
    assert_eq!(reopened.page, Page::Main);
}

#[test]
fn the_test_link_and_every_about_link_join_the_picker_queue_in_click_order() {
    let clicks = [
        Link::Test,
        Link::Source,
        Link::Test,
        Link::Issues,
        Link::License,
    ];
    let mut queue = LinkQueue::default();
    let mut presented = Vec::new();
    for link in clicks {
        let uri = Message::OpenLink(link).link().expect("a link message");
        if let Some(now) = queue.push(QueuedLink::new(uri)) {
            presented.push(now.uri);
        }
    }
    assert_eq!(presented.len(), 1, "one picker at a time; the rest wait");
    assert_eq!(queue.waiting(), clicks.len() - 1);
    loop {
        queue.closing();
        let Some(next) = queue.gone() else { break };
        presented.push(next.uri);
    }
    assert_eq!(presented, clicks.map(Link::uri));

    let everything = clicks.map(Link::uri);
    assert_eq!(
        valid_link_uris(&everything),
        everything,
        "the queue accepts every link the window sends"
    );
    assert_eq!(Message::Go(Page::About).link(), None);
    assert_eq!(Message::OpenAppDrawer("a".into()).link(), None);
}

#[test]
fn the_links_point_at_the_one_repository_constant() {
    assert_eq!(Link::Source.uri(), REPO_URL);
    assert_eq!(Link::Issues.uri(), format!("{REPO_URL}/issues"));
    assert_eq!(Link::Test.uri(), "https://example.org/");
    assert_eq!(Link::License.uri(), LICENSE_URL);
}

#[test]
fn opening_reads_the_snapshot_and_the_inventory() {
    let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    let window = world.open();
    let HandlerView::App(chrome) = &window.model.handler else {
        panic!("Chrome handles both schemes: {:?}", window.model.handler)
    };
    assert_eq!(
        (chrome.id.as_str(), chrome.name.as_str()),
        ("google-chrome.desktop", "Google Chrome")
    );
    assert_eq!(
        (window.model.action, window.model.can_run),
        (Action::UseSignpost, true)
    );
    assert_eq!(app_ids(&window), CHROME_FIRST);
}

#[test]
fn a_successful_set_refreshes_the_page_and_shows_its_undo_toast() {
    let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    let mut window = world.open();
    world.send(&mut window, Message::Run(Action::UseSignpost));
    assert_eq!(window.pending, Some(Op::Set));

    world.set_default().unwrap();
    world.send(&mut window, world.completion(Ok(())));

    assert_eq!(window.pending, None);
    assert_eq!(window.model.handler, HandlerView::Signpost);
    assert_eq!(
        (window.model.action, window.model.can_run),
        (Action::Restore, true)
    );
    assert_eq!(window.model.error_row, None);
    assert!(toast_shown(&window, "Signpost now opens your web links"));
    assert!(toast_shown(&window, "Undo"));
}

#[test]
fn a_failed_set_refreshes_the_page_and_shows_the_error_instead_of_a_toast() {
    let world = World::new(&[
        ("cosmic-mimeapps.list", CHROME_FOR_HTTPS),
        ("mimeapps.list", CHROME_FOR_WEB),
    ]);
    let mut window = world.open();
    world.send(&mut window, Message::Run(Action::UseSignpost));

    let failure = world.set_default().unwrap_err();
    assert_eq!(failure, OpError::Verify);
    world.send(&mut window, world.completion(Err(failure)));

    assert_eq!(window.pending, None);
    let error = window.model.error_row.as_ref().expect("the error row");
    assert_eq!(
        plain(&error.title),
        "Another settings file overrides this change"
    );
    assert_eq!(
        error.details,
        [format!(
            "{}: x-scheme-handler/https=google-chrome.desktop",
            world.list("cosmic-mimeapps.list").display()
        )]
    );
    assert!(error.offers_restore);
    assert_eq!(
        (window.model.action, window.model.can_run),
        (Action::UseSignpost, true),
        "the part that took effect leaves the row offering Use Signpost again"
    );
    assert!(window.model.toast.is_none());
    assert!(!toast_shown(&window, "Signpost now opens your web links"));
}

#[test]
fn a_new_result_collapses_the_details_and_a_successful_retry_clears_the_error() {
    let world = World::new(&[
        ("cosmic-mimeapps.list", CHROME_FOR_HTTPS),
        ("mimeapps.list", CHROME_FOR_WEB),
    ]);
    let mut window = world.open();
    let disk_full = || OpError::Failed("disk full".into());
    world.send(&mut window, Message::Run(Action::UseSignpost));
    world.send(&mut window, world.completion(Err(disk_full())));
    world.send(&mut window, Message::ToggleDetails);
    assert!(window.details_open);
    world.send(&mut window, Message::Run(Action::UseSignpost));
    world.send(&mut window, world.completion(Err(disk_full())));
    assert!(!window.details_open, "a new result starts collapsed");
    assert_eq!(
        window
            .model
            .error_row
            .as_ref()
            .map(|row| row.title.as_str()),
        Some("disk full")
    );

    std::fs::remove_file(world.list("cosmic-mimeapps.list")).unwrap();
    world.send(&mut window, Message::Run(Action::UseSignpost));
    world.set_default().unwrap();
    world.send(&mut window, world.completion(Ok(())));
    assert_eq!(window.model.error_row, None);
    assert_eq!(window.model.handler, HandlerView::Signpost);
}

#[test]
fn a_refresh_follows_the_files_when_the_index_changes() {
    let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    let mut window = world.open();
    assert!(matches!(window.model.handler, HandlerView::App(_)));

    std::fs::write(world.list("mimeapps.list"), "[Default Applications]\n").unwrap();
    window.refresh(&world.sources());
    assert_eq!(window.model.handler, HandlerView::None);

    world.set_default().unwrap();
    window.refresh(&world.sources());
    assert_eq!(window.model.handler, HandlerView::Signpost);
    assert_eq!(
        window.model.toast, None,
        "a refresh is not an operation result"
    );
}

#[test]
fn an_unreadable_record_is_read_again_on_refresh() {
    let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    let mut window = world.open();
    assert_eq!(window.model.record_error, None);

    std::fs::create_dir_all(world.state.join("signpost")).unwrap();
    std::fs::write(world.state.join("signpost/restore.json"), "{").unwrap();
    window.refresh(&world.sources());
    assert!(window.model.record_error.is_some());
}

#[test]
fn no_second_operation_starts_while_one_runs_and_a_result_frees_the_window() {
    let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    let mut window = world.open();
    assert!(!window.is_busy());
    world.send(&mut window, Message::Run(Action::UseSignpost));
    assert_eq!(window.pending, Some(Op::Set));
    world.send(&mut window, Message::Run(Action::Restore));
    assert_eq!(window.pending, Some(Op::Set), "the second press is ignored");
    assert!(window.is_busy());
    world.send(&mut window, world.completion(Ok(())));
    assert!(!window.is_busy());
}

#[test]
fn every_handler_row_action_maps_to_its_operation() {
    assert_eq!(Action::UseSignpost.op(), Op::Set);
    assert_eq!(Action::Restore.op(), Op::Restore);

    for (action, op) in [
        (Action::UseSignpost, Op::Set),
        (Action::Restore, Op::Restore),
    ] {
        let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
        let mut window = world.open();
        world.send(&mut window, Message::Run(action));
        assert_eq!(window.pending, Some(op), "{action:?}");
    }
}

#[test]
fn undo_runs_restore_and_removes_its_toast() {
    let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    let mut window = world.open();
    world.send(&mut window, Message::Run(Action::UseSignpost));
    world.set_default().unwrap();
    world.send(&mut window, world.completion(Ok(())));
    let undo = window.model.toast.as_ref().and_then(|toast| toast.undo);
    assert_eq!(undo, Some(Action::Restore));
    assert!(toast_shown(&window, "Signpost now opens your web links"));

    world.send(&mut window, Message::Undo(undo.unwrap()));

    assert_eq!(window.pending, Some(Op::Restore));
    assert!(!toast_shown(&window, "Signpost now opens your web links"));
}

#[test]
fn a_new_result_replaces_the_toast_still_showing() {
    let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    let mut window = world.open();
    world.send(&mut window, Message::Run(Action::UseSignpost));
    world.set_default().unwrap();
    world.send(&mut window, world.completion(Ok(())));
    assert!(toast_shown(&window, "Signpost now opens your web links"));

    world.send(&mut window, Message::Run(Action::Restore));
    world.restore().unwrap();
    world.send(&mut window, world.completion(Ok(())));

    assert!(toast_shown(
        &window,
        "Google Chrome now opens your web links"
    ));
    assert!(!toast_shown(&window, "Signpost now opens your web links"));
    assert!(!toast_shown(&window, "Undo"));
    assert!(matches!(window.model.handler, HandlerView::App(_)));
}

#[test]
fn an_operation_runs_the_setup_it_names_and_reports_failures_as_page_errors() {
    let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    assert!(
        matches!(world.restore(), Err(OpError::Failed(text)) if plain(&text) == "There's no previous browser to restore")
    );
    world.set_default().unwrap();
    assert!(setup::is_default_browser(&world.lists));
    world.restore().unwrap();
    assert!(!setup::is_default_browser(&world.lists));
}

#[test]
fn resizing_the_window_sets_condensed_and_other_windows_are_ignored() {
    let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    let id = window::Id::unique();
    let mut window = Window::open(id, DARK, &world.sources());
    assert!(!window.condensed);
    for (width, condensed) in [(360.0, true), (640.0, false), (479.0, true), (480.0, false)] {
        world.send(&mut window, Message::Resized(id, Size::new(width, 600.0)));
        assert_eq!(window.condensed, condensed, "{width} wide");
    }
    world.send(
        &mut window,
        Message::Resized(window::Id::unique(), Size::new(300.0, 300.0)),
    );
    assert!(!window.condensed, "a picker resizing is not this window");

    world.send(&mut window, Message::Maximized(true));
    assert!(window.maximized);
    world.send(&mut window, Message::Maximized(false));
    assert!(!window.maximized);
}

/// Whether `window` holds the shared handle of the About hero drawn for `dark`.
fn shows_hero_for(window: &Window, dark: bool) -> bool {
    let shared = icons::illustration(Illustration::AboutHero, dark);
    window.hero == shared && std::ptr::eq(window.hero.data(), shared.data())
}

#[test]
fn a_window_opens_with_the_hero_of_the_themes_tone() {
    let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    for dark in [true, false] {
        let window = Window::open(window::Id::unique(), dark, &world.sources());
        assert!(shows_hero_for(&window, dark), "opened with dark={dark}");
    }
}

#[test]
fn a_theme_change_swaps_the_hero_to_the_new_tone_and_back_without_rebuilding_it() {
    let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    let mut window = world.open();
    assert!(shows_hero_for(&window, true));
    window.retheme(false);
    assert!(shows_hero_for(&window, false));
    assert!(!shows_hero_for(&window, true));
    window.retheme(false);
    assert!(shows_hero_for(&window, false), "the same tone again");
    window.retheme(true);
    assert!(shows_hero_for(&window, true));
}

#[test]
fn the_window_is_a_sized_undecorated_surface_of_the_app() {
    let settings = window_settings();
    assert_eq!(settings.size, Size::new(640.0, 600.0));
    assert_eq!(settings.min_size, Some(Size::new(360.0, 400.0)));
    assert!(!settings.decorations);
    assert_eq!(settings.platform_specific.application_id, APP_ID);
}

#[test]
fn each_button_says_what_it_does() {
    for (action, label) in [
        (Action::UseSignpost, "Use Signpost"),
        (Action::Restore, "Restore defaults"),
    ] {
        assert_eq!(action.label(), label);
    }
}

#[test]
fn every_result_orders_the_apps_by_the_defaults_it_just_wrote_without_waiting_for_the_index() {
    let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    let mut window = world.open();
    assert_eq!(app_ids(&window), CHROME_FIRST);

    world.send(&mut window, Message::Run(Action::UseSignpost));
    world.set_default().unwrap();
    world.send(&mut window, world.completion(Ok(())));
    assert_eq!(app_ids(&window), BY_NAME);

    world.send(&mut window, Message::Run(Action::Restore));
    world.restore().unwrap();
    world.send(&mut window, world.completion(Ok(())));
    assert_eq!(app_ids(&window), CHROME_FIRST);
}

#[test]
fn a_failed_result_also_orders_the_apps_by_the_defaults_on_disk() {
    let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    let mut window = world.open();
    world.send(&mut window, Message::Run(Action::UseSignpost));

    std::fs::write(
        world.list("mimeapps.list"),
        "[Default Applications]\nx-scheme-handler/https=term.desktop\n",
    )
    .unwrap();
    let failure = OpError::Failed("disk full".into());
    world.send(&mut window, world.completion(Err(failure)));

    assert_eq!(app_ids(&window)[0], "term");
}

#[test]
fn opening_orders_the_apps_by_the_defaults_on_disk_now() {
    let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    world.set_default().unwrap();
    assert_eq!(app_ids(&world.open()), BY_NAME);
}

/// The tasks the buttons start change the files themselves, and their reports settle the page.
#[tokio::test(start_paused = true)]
async fn the_buttons_own_tasks_set_up_signpost_and_put_the_previous_browser_back() {
    let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    let mut window = world.open();
    for (action, signpost, next) in [
        (Action::UseSignpost, true, Action::Restore),
        (Action::Restore, false, Action::UseSignpost),
    ] {
        let reports = sent(world.deliver(&mut window, Message::Run(action))).await;
        assert!(
            matches!(reports.as_slice(), [Message::Done { result: Ok(()), .. }]),
            "{action:?}: {reports:?}"
        );
        for report in reports {
            world.send(&mut window, report);
        }
        assert_eq!(
            setup::is_default_browser(&world.lists),
            signpost,
            "{action:?}"
        );
        assert!(!window.is_busy(), "{action:?}");
        assert_eq!(window.model.action, next, "{action:?}");
    }
    let list = std::fs::read_to_string(world.list("mimeapps.list")).unwrap();
    assert!(
        list.contains("x-scheme-handler/https=google-chrome.desktop"),
        "{list}"
    );
}

#[tokio::test(start_paused = true)]
async fn an_earlier_toasts_timer_leaves_the_toast_that_replaced_it() {
    let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    let mut window = world.open();
    world.send(&mut window, Message::Run(Action::UseSignpost));
    world.set_default().unwrap();
    let first = world.deliver(&mut window, world.completion(Ok(())));
    world.send(&mut window, Message::Run(Action::Restore));
    world.restore().unwrap();
    world.send(&mut window, world.completion(Ok(())));

    for timer in sent(first).await {
        world.send(&mut window, timer);
    }

    assert!(toast_shown(
        &window,
        "Google Chrome now opens your web links"
    ));
}

#[tokio::test(start_paused = true)]
async fn a_closed_windows_toast_timer_leaves_the_toast_of_the_window_opened_after_it() {
    let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    let id = window::Id::unique();
    let mut slot = Some(Window::open(id, DARK, &world.sources()));
    let window = slot.as_mut().unwrap();
    world.send(window, Message::Run(Action::UseSignpost));
    world.set_default().unwrap();
    let first = world.deliver(window, world.completion(Ok(())));
    assert!(Window::closed(&mut slot, id));

    let mut reopened = world.open();
    world.send(&mut reopened, Message::Run(Action::Restore));
    world.restore().unwrap();
    world.send(&mut reopened, world.completion(Ok(())));
    for timer in sent(first).await {
        world.send(&mut reopened, timer);
    }

    assert!(toast_shown(
        &reopened,
        "Google Chrome now opens your web links"
    ));
}

#[tokio::test(start_paused = true)]
async fn a_toasts_own_timer_removes_it() {
    let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    let mut window = world.open();
    world.send(&mut window, Message::Run(Action::UseSignpost));
    world.set_default().unwrap();
    let timers = world.deliver(&mut window, world.completion(Ok(())));
    assert!(toast_shown(&window, "Signpost now opens your web links"));

    for timer in sent(timers).await {
        world.send(&mut window, timer);
    }

    assert!(!toast_shown(&window, "Signpost now opens your web links"));
}

#[tokio::test(start_paused = true)]
async fn a_full_queue_toasts_that_too_many_links_are_waiting_until_its_timer_ends() {
    let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    let mut window = world.open();
    let timers = window.links_waiting(&world.sources());
    assert!(toast_shown(&window, "Too many links are waiting"));

    for timer in sent(timers).await {
        world.send(&mut window, timer);
    }

    assert!(!toast_shown(&window, "Too many links are waiting"));
}

#[tokio::test(start_paused = true)]
async fn the_timer_of_a_links_toast_leaves_the_toast_that_replaced_it() {
    let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    let mut window = world.open();
    let first = window.links_waiting(&world.sources());
    assert!(toast_shown(&window, "Too many links are waiting"));
    world.send(&mut window, Message::Run(Action::UseSignpost));
    world.set_default().unwrap();
    world.send(&mut window, world.completion(Ok(())));

    for timer in sent(first).await {
        world.send(&mut window, timer);
    }

    assert!(toast_shown(&window, "Signpost now opens your web links"));
}

#[test]
fn an_operation_outlives_its_window_and_the_window_opened_next_shows_it_pending() {
    let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    let id = window::Id::unique();
    let mut slot = Some(Window::open(id, DARK, &world.sources()));
    world.send(slot.as_mut().unwrap(), Message::Run(Action::UseSignpost));
    assert!(Window::closed(&mut slot, id));

    let mut reopened = world.open();
    assert!(reopened.is_busy(), "the operation is still running");
    world.send(&mut reopened, Message::Run(Action::Restore));
    assert_eq!(reopened.pending, Some(Op::Set), "a second run is refused");
}

#[test]
fn a_late_completion_cannot_free_a_newer_operation() {
    let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    let mut window = world.open();
    world.send(&mut window, Message::Run(Action::UseSignpost));
    let first = world.completion(Ok(()));
    world.send(&mut window, first.clone());
    world.send(&mut window, Message::Run(Action::Restore));
    assert_eq!(window.pending, Some(Op::Restore));

    world.send(&mut window, first);

    assert_eq!(window.pending, Some(Op::Restore), "still the newer one");
    assert_eq!(world.ops.running(), Some(Op::Restore));
    assert_eq!(
        window.last.as_ref().map(|(op, _)| *op),
        Some(Op::Set),
        "the late completion is not a new result"
    );
    world.send(&mut window, Message::Run(Action::UseSignpost));
    assert_eq!(
        window.pending,
        Some(Op::Restore),
        "and it still refuses a third"
    );
}

#[test]
fn a_completion_with_no_window_open_frees_the_operation_and_the_next_window_reads_the_files() {
    let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    let id = window::Id::unique();
    let mut slot = Some(Window::open(id, DARK, &world.sources()));
    world.send(slot.as_mut().unwrap(), Message::Run(Action::UseSignpost));
    let done = world.completion(Ok(()));
    assert!(Window::closed(&mut slot, id));
    world.set_default().unwrap();

    assert!(
        world.ops.settle(done).is_some(),
        "settled, though nothing shows it"
    );

    assert_eq!(world.ops.running(), None);
    let reopened = world.open();
    assert!(!reopened.is_busy());
    assert_eq!(reopened.model.handler, HandlerView::Signpost);
}

#[test]
fn a_window_opened_during_an_operation_shows_its_result_when_it_ends() {
    let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    let id = window::Id::unique();
    let mut slot = Some(Window::open(id, DARK, &world.sources()));
    world.send(slot.as_mut().unwrap(), Message::Run(Action::UseSignpost));
    assert!(Window::closed(&mut slot, id));
    let mut reopened = world.open();
    world.set_default().unwrap();

    world.send(&mut reopened, world.completion(Ok(())));

    assert!(!reopened.is_busy());
    assert_eq!(reopened.model.handler, HandlerView::Signpost);
    assert!(toast_shown(&reopened, "Signpost now opens your web links"));
}

#[tokio::test]
async fn a_setup_task_that_panics_ends_as_an_error_and_the_next_run_is_accepted() {
    let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    let mut window = world.open();
    let task = window.start(Op::Set, &world.sources(), |_, _| panic!("setup bug"));
    assert_eq!(world.ops.running(), Some(Op::Set));

    let emitted = sent(task).await;

    assert!(
        matches!(
            emitted.as_slice(),
            [Message::Done {
                result: Err(OpError::Task),
                ..
            }]
        ),
        "the task reports its failure once: {emitted:?}"
    );
    for message in emitted {
        world.send(&mut window, message);
    }
    assert_eq!(world.ops.running(), None, "the ledger is free again");
    assert!(!window.is_busy());
    let error = window.model.error_row.as_ref().expect("the error row");
    assert_eq!(error.title, "The setup task failed");

    world.send(&mut window, Message::Run(Action::Restore));
    assert_eq!(window.pending, Some(Op::Restore), "a following run starts");
}

const TEAL: Rgb = Rgb {
    r: 0x12,
    g: 0xA5,
    b: 0x94,
};
const CHROME_APP: &str = "google-chrome";

fn drawer_app(window: &Window) -> Option<&str> {
    window.drawer.as_ref().map(|drawer| drawer.app.as_str())
}

fn open_swatch(window: &Window) -> Option<&str> {
    window.drawer.as_ref()?.swatch.as_deref()
}

fn open_drawer(world: &World) -> Window {
    let mut window = world.open();
    world.send(&mut window, Message::OpenAppDrawer(CHROME_APP.to_owned()));
    window
}

fn key_press(key: keyboard::key::Named, code: keyboard::key::Code) -> keyboard::Event {
    let key = keyboard::Key::Named(key);
    keyboard::Event::KeyPressed {
        key: key.clone(),
        modified_key: key,
        physical_key: keyboard::key::Physical::Code(code),
        location: keyboard::Location::Standard,
        modifiers: keyboard::Modifiers::empty(),
        text: None,
        repeat: false,
    }
}

fn escape() -> keyboard::Event {
    key_press(keyboard::key::Named::Escape, keyboard::key::Code::Escape)
}

#[test]
fn pressing_an_app_opens_its_drawer_and_a_press_on_an_app_no_longer_listed_changes_nothing() {
    let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    let mut window = world.open();
    assert_eq!(drawer_app(&window), None);

    world.send(&mut window, Message::OpenAppDrawer(CHROME_APP.to_owned()));
    assert_eq!(drawer_app(&window), Some(CHROME_APP));

    world.send(&mut window, Message::OpenAppDrawer("gone".to_owned()));
    assert_eq!(drawer_app(&window), Some(CHROME_APP), "a stale press");

    world.send(&mut window, Message::Swatch(Swatch::Open("Default".into())));
    world.send(&mut window, Message::OpenAppDrawer("firefox".to_owned()));
    assert_eq!(drawer_app(&window), Some("firefox"));
    assert_eq!(open_swatch(&window), None, "another app starts clean");
}

#[test]
fn the_drawers_close_button_and_leaving_the_main_page_close_it() {
    let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    let mut window = open_drawer(&world);
    world.send(&mut window, Message::CloseAppDrawer);
    assert_eq!(drawer_app(&window), None);

    for page in [Page::About, Page::Shortcuts] {
        let mut window = open_drawer(&world);
        world.send(&mut window, Message::Go(page));
        assert_eq!(drawer_app(&window), None, "{page:?}");
        world.send(&mut window, Message::Go(Page::Main));
        assert_eq!(drawer_app(&window), None, "coming back does not reopen it");
    }
}

#[test]
fn a_refresh_keeps_the_open_drawer_and_closes_it_when_its_app_disappears() {
    let mut world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    let mut window = open_drawer(&world);
    world.send(&mut window, Message::Swatch(Swatch::Open("Default".into())));

    window.refresh(&world.sources());
    assert_eq!(drawer_app(&window), Some(CHROME_APP));
    assert_eq!(open_swatch(&window), Some("Default"));

    world.registry = Registry::load(&[fixtures().join("applications-user")], &["en".into()]);
    window.refresh(&world.sources());
    assert!(!app_ids(&window).contains(&CHROME_APP));
    assert_eq!(drawer_app(&window), None);
}

#[test]
fn choosing_a_color_in_the_drawer_reaches_memory_and_disk_and_the_drawers_own_content() {
    let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    let mut window = open_drawer(&world);
    let key = override_key(CHROME_APP, "Default");

    world.send(&mut window, Message::Swatch(Swatch::Open("Default".into())));
    world.send(&mut window, Message::Swatch(Swatch::Choose(Some(TEAL))));

    assert_eq!(world.colors.borrow().get(&key), Some(TEAL));
    assert_eq!(world.color_store.load().unwrap().get(&key), Some(TEAL));
    let chrome = window
        .inventory
        .iter()
        .find(|a| a.id == CHROME_APP)
        .cloned()
        .unwrap();
    let shown = drawer::content(&chrome, &world.colors.borrow());
    assert_eq!(
        shown.profiles[0].color,
        Some(TEAL),
        "over the browser's own"
    );

    world.send(&mut window, Message::Swatch(Swatch::Open("Default".into())));
    world.send(&mut window, Message::Swatch(Swatch::Choose(None)));
    assert_eq!(world.colors.borrow().get(&key), None);
    let shown = drawer::content(&chrome, &world.colors.borrow());
    assert_eq!(
        shown.profiles[0].color, chrome.profiles[0].seed,
        "reset returns to the seed"
    );
}

#[test]
fn escape_dismisses_the_color_popover_of_this_window_only_while_one_is_open() {
    let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    let id = window::Id::unique();
    let mut window = Window::open(id, DARK, &world.sources());
    world.send(&mut window, Message::OpenAppDrawer(CHROME_APP.to_owned()));
    let dismissal = |window: &Window, id, event: &keyboard::Event| window.dismiss(id, event);
    assert!(
        dismissal(&window, id, &escape()).is_none(),
        "nothing to dismiss"
    );

    world.send(&mut window, Message::Swatch(Swatch::Open("Default".into())));
    let enter = key_press(keyboard::key::Named::Enter, keyboard::key::Code::Enter);
    assert!(dismissal(&window, id, &enter).is_none(), "only Esc");
    assert!(
        dismissal(&window, window::Id::unique(), &escape()).is_none(),
        "a picker's Esc is not this window's"
    );
    let message = dismissal(&window, id, &escape()).expect("Esc closes the popover");

    world.send(&mut window, message);
    assert_eq!(open_swatch(&window), None);
    assert_eq!(drawer_app(&window), Some(CHROME_APP), "the drawer stays");
}

#[test]
fn a_flatpak_that_cannot_reach_the_users_list_sets_up_nothing() {
    let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    let before = std::fs::read_to_string(world.list("mimeapps.list")).unwrap();
    let files = Files {
        user_list: None,
        ..world.files()
    };
    let Err(OpError::Failed(reason)) = execute(Op::Set, &files) else {
        panic!("a write error");
    };
    assert!(
        plain(&reason).contains("flatpak override --user --filesystem="),
        "it names the remedy: {reason}"
    );
    assert_eq!(
        std::fs::read_to_string(world.list("mimeapps.list")).unwrap(),
        before
    );
    assert!(
        !world.state.join("signpost").exists(),
        "no record was written"
    );
}

#[test]
fn a_list_read_through_the_sandbox_is_shown_by_its_host_path() {
    let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
    let root = tempfile::tempdir().unwrap();
    let system = root.path().join("os/etc/xdg/mimeapps.list");
    std::fs::create_dir_all(system.parent().unwrap()).unwrap();
    std::fs::write(&system, CHROME_FOR_HTTPS).unwrap();
    let host = Host::Flatpak(
        crate::host::HostEnv::parse(b"HOME=/home/u\0", Path::new("/sandbox/home"))
            .mounted_under(root.path()),
    );
    let sources = Sources {
        lists: vec![system, world.list("mimeapps.list")],
        host: &host,
        ..world.sources()
    };
    let (_, model) = facts(&sources, Some(&(Op::Set, Err(OpError::Verify))));
    let details = model.error_row.expect("the error row").details;
    assert!(
        details.contains(
            &"/etc/xdg/mimeapps.list: x-scheme-handler/https=google-chrome.desktop".to_owned()
        ),
        "{details:?}"
    );
    let shown_in = root.path().to_string_lossy();
    assert!(
        details.iter().all(|line| !line.contains(shown_in.as_ref())),
        "never a path where the sandbox reads it: {details:?}"
    );
}
