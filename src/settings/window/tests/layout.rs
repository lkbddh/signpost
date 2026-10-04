//! The window drawn and driven with the software renderer: no window, no compositor.

use cosmic::Element;
use cosmic::iced::mouse::ScrollDelta;
use cosmic::iced::{Color, Event, Point, Rectangle, Size, Vector, mouse};
use cosmic::widget::svg;

use super::*;
use crate::test_support::headless::{self, DrawnSvg, Frames, channel_gap};

const SCRATCH_HOME: &str = "SIGNPOST_WINDOW_SCRATCH_HOME";
/// The window's smallest size, which the app list is longer than.
const SMALLEST: Size = Size::new(360.0, 400.0);
/// Over the page, below the header bar.
const OVER_PAGE: Point = Point::new(20.0, 300.0);
/// Pixels the wheel turns to scroll the page down.
const WHEEL_DOWN: f32 = -100.0;
/// The window's smallest, its opening size, a wide one, and two wider than the hero is tall.
const WINDOW_SIZES: [Size; 5] = [
    SMALLEST,
    Size::new(640.0, 600.0),
    Size::new(1280.0, 720.0),
    Size::new(1920.0, 600.0),
    Size::new(3000.0, 800.0),
];
/// How tall the hero is for each pixel of width: its file's `viewBox` is 640 by 240.
const HERO_HEIGHT_PER_WIDTH: f32 = 240.0 / 640.0;
/// The header bar above the page, at the default density.
const HEADER_HEIGHT: f32 = 47.0;
/// How far a drawn edge may sit from the exact one.
const ONE_PIXEL: f32 = 1.0;
/// The window's opening size.
const OPENING: Size = Size::new(640.0, 600.0);
/// The title of the error band setup leaves when another settings file overrides it.
const OVERRIDDEN: &str = "Another settings file overrides this change";

/// Runs test `name` again with its home in a scratch directory, and says whether this is that run.
fn in_scratch_home(name: &str) -> bool {
    headless::in_scratch_home(
        SCRATCH_HOME,
        &format!("settings::window::tests::layout::{name}"),
    )
}

fn is_within_a_pixel(drawn: Rectangle, expected: Rectangle) -> bool {
    [
        drawn.x - expected.x,
        drawn.y - expected.y,
        drawn.width - expected.width,
        drawn.height - expected.height,
    ]
    .iter()
    .all(|gap| gap.abs() <= ONE_PIXEL)
}

/// Where a window of `size` draws its hero on the About page that scrolls in `page`: across the window, on the
/// page's bottom edge; and the part of that the page shows, scrolled to its top.
fn expected_hero(size: Size, page: &headless::Scrolled) -> (Rectangle, Rectangle) {
    let height = size.width * HERO_HEIGHT_PER_WIDTH;
    let bottom = page.content.y + page.content.height;
    let whole = Rectangle::new(
        Point::new(0.0, bottom - height),
        Size::new(size.width, height),
    );
    let shown = whole
        .intersection(&page.bounds)
        .expect("the hero is on the page");
    (whole, shown)
}

/// A window as the app shows it, and the frames drawn of it.
struct Scene {
    world: World,
    window: Window,
    frames: Frames,
}

impl Scene {
    fn new() -> Self {
        Self::sized(SMALLEST)
    }

    fn sized(size: Size) -> Self {
        let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
        let window = world.open();
        Self {
            world,
            window,
            frames: Frames::new(size),
        }
    }

    /// A window of `size` after setup was overridden by another settings file: its error band has a hint,
    /// details and Restore.
    fn overridden(size: Size) -> Self {
        let world = World::new(&[
            ("cosmic-mimeapps.list", CHROME_FOR_HTTPS),
            ("mimeapps.list", CHROME_FOR_WEB),
        ]);
        let mut window = world.open();
        world.send(&mut window, Message::Run(Action::UseSignpost));
        let failure = world.set_default().unwrap_err();
        world.send(&mut window, world.completion(Err(failure)));
        Self {
            world,
            window,
            frames: Frames::new(size),
        }
    }

    /// A window of `size` whose record of the saved defaults cannot be read.
    fn unreadable(size: Size) -> Self {
        let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
        std::fs::create_dir_all(world.state.join("signpost")).unwrap();
        std::fs::write(world.state.join("signpost/restore.json"), "{").unwrap();
        let window = world.open();
        Self {
            world,
            window,
            frames: Frames::new(size),
        }
    }

    /// A window of `size` where Signpost opens HTTPS links and Chrome HTTP ones, after a setup it can
    /// restore.
    fn split(size: Size) -> Self {
        let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
        world.set_default().unwrap();
        std::fs::write(
            world.list("mimeapps.list"),
            format!(
                "[Default Applications]\nx-scheme-handler/http=google-chrome.desktop\n\
                 x-scheme-handler/https={}\n",
                crate::setup::DESKTOP_FILE
            ),
        )
        .unwrap();
        let window = world.open();
        Self {
            world,
            window,
            frames: Frames::new(size),
        }
    }

    /// A window of `size` where Chrome opens web links again after a setup Signpost can restore: the row offers Use
    /// Signpost, and a band from a failed setup offers Restore defaults.
    fn restorable(size: Size) -> Self {
        let world = World::new(&[("mimeapps.list", CHROME_FOR_WEB)]);
        world.set_default().unwrap();
        std::fs::write(world.list("mimeapps.list"), CHROME_FOR_WEB).unwrap();
        let window = world.open();
        Self {
            world,
            window,
            frames: Frames::new(size),
        }
    }

    /// This scene, with the window told its size as the runtime tells it.
    fn resized(mut self) -> Self {
        let size = self.frames.surface();
        self.send(Message::Resized(self.window.id(), size));
        self
    }

    /// This scene, after `messages`.
    fn after(mut self, messages: impl IntoIterator<Item = Message>) -> Self {
        for message in messages {
            self.send(message);
        }
        self
    }

    fn send(&mut self, message: Message) {
        self.world.send(&mut self.window, message);
    }

    /// The strings the window draws bigger than their bounds.
    fn overflowing(&mut self) -> Vec<(String, Rectangle, Rectangle)> {
        let colors = self.world.colors.borrow();
        self.frames
            .overflowing_texts(self.window.view(true, &colors))
    }

    /// Every string the page lays out.
    fn texts(&mut self) -> Vec<String> {
        let colors = self.world.colors.borrow();
        self.frames
            .probe(self.window.view(true, &colors))
            .texts
            .into_iter()
            .map(|(text, _)| text)
            .collect()
    }

    /// Where the error band titled `title` draws its title, its warning and its Restore.
    fn band(&mut self, title: &str) -> Band {
        let colors = self.world.colors.borrow();
        let texts = self.frames.probe(self.window.view(true, &colors)).texts;
        let at = texts
            .iter()
            .position(|(text, _)| text == title)
            .expect("the band's title");
        let restore = texts[at..]
            .iter()
            .find(|(text, _)| text == "Restore defaults")
            .expect("the band's Restore defaults");
        let warning = svg::Handle::from_memory(WARNING_ICON);
        Band {
            title: texts[at].1,
            warning: self
                .frames
                .drawn_svg_of(&warning, self.window.view(true, &colors))
                .bounds,
            restore: restore.1,
        }
    }

    /// The page's own scroll offset; it is the first scrollable drawn, under the drawer's.
    fn page_offset(&mut self) -> Vector {
        let colors = self.world.colors.borrow();
        let probe = self.frames.probe(self.window.view(true, &colors));
        probe.scrollables.first().expect("the page scrolls").offset
    }

    /// The About page's scroll: the page below the header bar, and all of it at its full height.
    fn about_page(&mut self) -> headless::Scrolled {
        let colors = self.world.colors.borrow();
        let mut probe = self.frames.probe(self.window.view(true, &colors));
        assert_eq!(probe.scrollables.len(), 1, "the About page scrolls as one");
        probe.scrollables.remove(0)
    }

    /// The handle the page draws its hero of the file `file` holds with.
    fn hero_drawn(&mut self, file: &svg::Handle) -> svg::Handle {
        let colors = self.world.colors.borrow();
        self.frames
            .drawn_copy_of(file, self.window.view(true, &colors))
    }

    fn scroll_page_down(&mut self) {
        let colors = self.world.colors.borrow();
        let events = [
            Event::Mouse(mouse::Event::CursorMoved {
                position: OVER_PAGE,
            }),
            Event::Mouse(mouse::Event::WheelScrolled {
                delta: ScrollDelta::Pixels {
                    x: 0.0,
                    y: WHEEL_DOWN,
                },
            }),
        ];
        self.frames
            .send(self.window.view(true, &colors), &events, OVER_PAGE);
    }

    /// The page's hero of the tone `dark`, as it draws it.
    fn hero(&mut self, dark: bool) -> DrawnSvg {
        let colors = self.world.colors.borrow();
        let file = icons::illustration(Illustration::AboutHero, dark);
        self.frames
            .drawn_svg_of(&file, self.window.view(true, &colors))
    }
}

/// The warning icon's file, as the band draws it.
const WARNING_ICON: &[u8] =
    include_bytes!("../../../../resources/icons/phosphor/warning-circle.svg");

#[derive(Debug, PartialEq)]
struct Band {
    title: Rectangle,
    warning: Rectangle,
    restore: Rectangle,
}

fn middle(bounds: Rectangle) -> f32 {
    bounds.y + bounds.height / 2.0
}

#[test]
fn the_error_band_keeps_its_warning_and_restore_on_its_titles_line_as_the_details_open() {
    if !in_scratch_home(
        "the_error_band_keeps_its_warning_and_restore_on_its_titles_line_as_the_details_open",
    ) {
        return;
    }
    let mut scene = Scene::overridden(OPENING);
    let collapsed = scene.band(OVERRIDDEN);
    scene.send(Message::ToggleDetails);
    assert!(scene.window.details_open);
    let expanded = scene.band(OVERRIDDEN);
    for (state, band) in [("collapsed", &collapsed), ("expanded", &expanded)] {
        for (part, bounds) in [
            ("warning", band.warning),
            ("Restore defaults", band.restore),
        ] {
            assert!(
                (middle(bounds) - middle(band.title)).abs() <= ONE_PIXEL,
                "{state}: the {part} at {bounds:?} is off the title's line, {:?}",
                band.title
            );
        }
    }
    assert_eq!(
        expanded, collapsed,
        "the details open below the title's line"
    );
}

#[test]
fn an_unreadable_record_is_named_in_a_short_title_with_its_path_behind_show_details() {
    if !in_scratch_home(
        "an_unreadable_record_is_named_in_a_short_title_with_its_path_behind_show_details",
    ) {
        return;
    }
    let mut scene = Scene::unreadable(OPENING);
    let texts = scene.texts();
    assert!(
        texts
            .iter()
            .any(|text| text == "Saved defaults couldn't be read"),
        "{texts:?}"
    );
    assert!(
        !texts.iter().any(|text| text.contains("restore.json")),
        "the path shows only in the details: {texts:?}"
    );
    let colors = scene.world.colors.borrow();
    let probe = scene.frames.probe(scene.window.view(true, &colors));
    let (_, button) = probe
        .texts
        .iter()
        .find(|(text, _)| text == "Use Signpost")
        .expect("the row keeps its button in its place");
    let center = button.center();
    let click = [
        Event::Mouse(mouse::Event::CursorMoved { position: center }),
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
    ];
    let sent = scene
        .frames
        .send(scene.window.view(true, &colors), &click, center);
    drop(colors);
    assert!(
        !sent
            .iter()
            .any(|message| matches!(message, Message::Run(_))),
        "setup cannot be started while it would fail: {sent:?}"
    );
    scene.send(Message::ToggleDetails);
    let texts = scene.texts();
    assert!(
        texts.iter().any(|text| text.contains("restore.json")),
        "Show details shows the path: {texts:?}"
    );
}

/// Every state of the window that holds long words or many controls, at `size`, each named.
fn states(size: Size) -> Vec<(&'static str, Scene)> {
    let palette = || {
        [
            Message::OpenAppDrawer(CHROME_APP.to_owned()),
            Message::Swatch(drawer::Swatch::Open("Default".into())),
        ]
    };
    vec![
        ("an app's default", Scene::sized(size)),
        ("an app's default after a setup", Scene::restorable(size)),
        ("a split default", Scene::split(size)),
        ("an overridden setup", Scene::overridden(size)),
        (
            "an overridden setup's details",
            Scene::overridden(size).after([Message::ToggleDetails]),
        ),
        ("an unreadable record", Scene::unreadable(size)),
        (
            "an unreadable record's details",
            Scene::unreadable(size).after([Message::ToggleDetails]),
        ),
        (
            "the shortcuts",
            Scene::sized(size).after([Message::Go(Page::Shortcuts)]),
        ),
        (
            "about",
            Scene::sized(size).after([Message::Go(Page::About)]),
        ),
        ("a color palette", Scene::sized(size).after(palette())),
    ]
    .into_iter()
    .map(|(state, scene)| (state, scene.resized()))
    .collect()
}

#[test]
fn no_text_is_cut_off_in_the_smallest_window_or_the_opening_one() {
    if !in_scratch_home("no_text_is_cut_off_in_the_smallest_window_or_the_opening_one") {
        return;
    }
    let mut cut = Vec::new();
    for size in [SMALLEST, Size::new(SMALLEST.width, 1600.0), OPENING] {
        for (state, mut scene) in states(size) {
            for (text, drawn, laid_out) in scene.overflowing() {
                cut.push(format!(
                    "{state} at {size:?}: {text:?} drawn at {drawn:?} in {laid_out:?}"
                ));
            }
        }
    }
    assert!(cut.is_empty(), "{} cut off:\n{}", cut.len(), cut.join("\n"));
}

/// The share of the hero above its skyline, which may fade behind the list: the tallest roof in its file
/// starts 104 of its 240 units down.
const SKY_SHARE: f32 = 104.0 / 240.0;

#[test]
fn the_about_list_ends_above_the_heros_skyline() {
    if !in_scratch_home("the_about_list_ends_above_the_heros_skyline") {
        return;
    }
    for size in WINDOW_SIZES {
        let mut scene = Scene::sized(size)
            .after([Message::Go(Page::About)])
            .resized();
        let colors = scene.world.colors.borrow();
        let probe = scene.frames.probe(scene.window.view(true, &colors));
        // The list and the hero scroll together, so the page's top is where both are measured from.
        let list_bottom = probe
            .stops
            .iter()
            .map(|stop| stop.y + stop.height)
            .fold(0.0, f32::max);
        drop(colors);
        for dark in [true, false] {
            scene.window.retheme(dark);
            let hero = scene.hero(dark).bounds;
            let skyline = hero.y + hero.height * SKY_SHARE;
            assert!(
                list_bottom <= skyline + ONE_PIXEL,
                "a window of {size:?}, dark={dark}: the list ends at {list_bottom}, over the skyline from {skyline}"
            );
        }
    }
}

#[test]
fn a_short_wide_window_shows_the_about_pages_way_back_and_keeps_its_facts_in_reach() {
    if !in_scratch_home(
        "a_short_wide_window_shows_the_about_pages_way_back_and_keeps_its_facts_in_reach",
    ) {
        return;
    }
    for size in [Size::new(1920.0, 400.0), Size::new(3000.0, 600.0)] {
        let mut scene = Scene::sized(size)
            .after([Message::Go(Page::About)])
            .resized();
        let colors = scene.world.colors.borrow();
        let probe = scene.frames.probe(scene.window.view(true, &colors));
        let page = Rectangle::new(
            Point::new(0.0, HEADER_HEIGHT),
            Size::new(size.width, size.height - HEADER_HEIGHT),
        );
        // The Back button's label repeats the header bar's title, which comes first.
        let text = |wanted: &str| {
            probe
                .texts
                .iter()
                .rev()
                .find(|(text, _)| text == wanted)
                .map(|(_, bounds)| *bounds)
                .expect("the text")
        };
        for shown in ["Signpost", "About Signpost"] {
            let bounds = text(shown);
            assert!(
                bounds.height > 0.0 && headless::lies_inside(bounds, page),
                "a window of {size:?}: {shown:?} at {bounds:?} does not show on the page, {page:?}"
            );
        }
        let reach = probe
            .scrollables
            .iter()
            .map(|scrolled| scrolled.content)
            .collect::<Vec<_>>();
        for fact in ["Version", "Source code", "Report an issue", "License"] {
            let bounds = text(fact);
            assert!(
                bounds.height > 0.0
                    && reach
                        .iter()
                        .any(|content| headless::lies_inside(bounds, *content)),
                "a window of {size:?}: {fact:?} at {bounds:?} cannot be scrolled to, {reach:?}"
            );
        }
    }
}

#[test]
fn the_shortcuts_keys_stay_on_one_line_in_the_smallest_window() {
    if !in_scratch_home("the_shortcuts_keys_stay_on_one_line_in_the_smallest_window") {
        return;
    }
    let mut scene = Scene::new().after([Message::Go(Page::Shortcuts)]).resized();
    let colors = scene.world.colors.borrow();
    let texts = scene.frames.probe(scene.window.view(true, &colors)).texts;
    let height_of = |key: &str| {
        texts
            .iter()
            .find(|(text, _)| text == key)
            .map(|(_, bounds)| bounds.height)
            .expect("the key")
    };
    let line = height_of("C");
    for key in ["Arrow keys", "1–9", "Enter", "Ctrl", "Esc"] {
        assert!((height_of(key) - line).abs() <= ONE_PIXEL, "{key} wraps");
    }
}

#[test]
fn the_color_palette_and_every_color_in_it_stay_inside_the_window() {
    if !in_scratch_home("the_color_palette_and_every_color_in_it_stay_inside_the_window") {
        return;
    }
    for size in [SMALLEST, OPENING] {
        let mut scene = Scene::sized(size)
            .after([
                Message::OpenAppDrawer(CHROME_APP.to_owned()),
                Message::Swatch(drawer::Swatch::Open("Default".into())),
            ])
            .resized();
        let colors = scene.world.colors.borrow();
        let probe = scene.frames.probe(scene.window.view(true, &colors));
        let popup = probe
            .container_named(&drawer::PALETTE_POPUP)
            .expect("the palette");
        let window = Rectangle::new(Point::ORIGIN, size);
        assert!(
            headless::lies_inside(popup, window),
            "a window of {size:?}: the palette at {popup:?} leaves it"
        );
        // The palette's own controls, not those of the drawer under it.
        let controls: Vec<Rectangle> = probe
            .stops
            .iter()
            .zip(&probe.stops_enclosing)
            .filter(|(_, holders)| holders.contains(&popup))
            .map(|(stop, _)| *stop)
            .collect();
        assert_eq!(
            controls.len(),
            crate::colors::PALETTE.len() + 1,
            "a window of {size:?}: a control for every color, and Reset"
        );
        for control in controls {
            assert!(
                headless::lies_inside(control, popup),
                "a window of {size:?}: a control at {control:?} leaves the palette at {popup:?}"
            );
        }
    }
}

#[test]
fn the_about_page_stretches_its_hero_across_the_window_at_every_size() {
    if !in_scratch_home("the_about_page_stretches_its_hero_across_the_window_at_every_size") {
        return;
    }
    for size in WINDOW_SIZES {
        let mut scene = Scene::sized(size);
        scene.send(Message::Resized(scene.window.id(), size));
        scene.page_offset();
        scene.send(Message::Go(Page::About));
        let page = scene.about_page();
        assert!(
            page.content.height >= page.bounds.height - ONE_PIXEL,
            "a window of {size:?}: the page, {:?}, is shorter than the window, {:?}",
            page.content,
            page.bounds
        );
        let (whole, shown) = expected_hero(size, &page);
        for dark in [true, false] {
            scene.window.retheme(dark);
            let hero = scene.hero(dark);
            assert!(
                is_within_a_pixel(hero.bounds, whole),
                "a window of {size:?}, dark={dark}: the hero is drawn at {:?}, not {whole:?}",
                hero.bounds
            );
            assert!(
                is_within_a_pixel(hero.visible, shown),
                "a window of {size:?}, dark={dark}: the hero shows at {:?}, not {shown:?}",
                hero.visible
            );
        }
    }
}

#[test]
fn opening_and_closing_an_apps_drawer_leaves_the_page_scrolled_where_it_was() {
    if !in_scratch_home("opening_and_closing_an_apps_drawer_leaves_the_page_scrolled_where_it_was")
    {
        return;
    }
    let mut scene = Scene::new();
    assert_eq!(scene.page_offset(), Vector::ZERO, "starts at the top");
    scene.scroll_page_down();
    let scrolled = scene.page_offset();
    assert!(scrolled.y > 0.0, "the app list scrolls: {scrolled:?}");

    scene.send(Message::OpenAppDrawer(CHROME_APP.to_owned()));
    assert_eq!(scene.page_offset(), scrolled, "with the drawer open");

    scene.send(Message::CloseAppDrawer);
    assert_eq!(
        scene.page_offset(),
        scrolled,
        "with the drawer closed again"
    );
}

#[test]
fn the_about_page_draws_the_shared_hero_of_the_themes_tone_in_every_build() {
    if !in_scratch_home("the_about_page_draws_the_shared_hero_of_the_themes_tone_in_every_build") {
        return;
    }
    let mut scene = Scene::new();
    scene.send(Message::Go(Page::About));
    for dark in [true, false] {
        scene.window.retheme(dark);
        let shared = icons::illustration(Illustration::AboutHero, dark);
        for build in 1..=2 {
            let drawn = scene.hero_drawn(&shared);
            assert!(
                std::ptr::eq(drawn.data(), shared.data()),
                "build {build} with dark={dark} drew a hero of its own"
            );
        }
    }
}

#[test]
fn the_main_page_names_its_default_as_cosmic_does_without_a_heading_over_it() {
    if !in_scratch_home("the_main_page_names_its_default_as_cosmic_does_without_a_heading_over_it")
    {
        return;
    }
    let mut scene = Scene::new();
    let colors = scene.world.colors.borrow();
    let texts: Vec<String> = scene
        .frames
        .probe(scene.window.view(true, &colors))
        .texts
        .into_iter()
        .map(|(text, _)| text)
        .collect();
    assert!(
        texts.iter().any(|text| text == "Web browser"),
        "the default's row is called Web browser: {texts:?}"
    );
    assert!(
        texts.iter().any(|text| text == "Use Signpost"),
        "the row says what its button does: {texts:?}"
    );
    for gone in ["Web links", "Default handler", "Set as default", "Restore"] {
        assert!(
            !texts.iter().any(|text| text == gone),
            "the page still draws {gone:?}: {texts:?}"
        );
    }
}

#[test]
fn a_record_that_breaks_after_the_window_opens_never_shows_its_path_until_asked() {
    if !in_scratch_home(
        "a_record_that_breaks_after_the_window_opens_never_shows_its_path_until_asked",
    ) {
        return;
    }
    let mut scene = Scene::sized(OPENING);
    let state = scene.world.state.join("signpost");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join("restore.json"), "{").unwrap();
    scene.send(Message::Run(Action::UseSignpost));
    let failure = scene.world.set_default().unwrap_err();
    assert!(matches!(failure, OpError::Record(_)), "{failure:?}");
    let done = scene.world.completion(Err(failure));
    scene.send(done);
    let texts = scene.texts();
    assert!(
        !texts.iter().any(|text| text.contains("restore.json")),
        "the path shows only in the details: {texts:?}"
    );
    assert_eq!(
        texts
            .iter()
            .filter(|text| *text == "Saved defaults couldn't be read")
            .count(),
        1,
        "one band says it: {texts:?}"
    );
}

/// The steepest a fading sky may change between two neighboring pixels: no step shows.
const GENTLE: f32 = 3.0 / 255.0;
/// More than a renderer's rounding; the night sky is close to a dark window, so its tint is faint.
const TINTED: f32 = 1.5 / 255.0;

#[test]
fn every_illustration_is_opaque_so_every_renderer_draws_it_as_its_file_says() {
    if !in_scratch_home("every_illustration_is_opaque_so_every_renderer_draws_it_as_its_file_says")
    {
        return;
    }
    // The GPU renderer blends an SVG's partly transparent pixels darker than they are, so the illustrations leave
    // their fades to the window.
    for (which, size) in [
        (Illustration::AboutHero, Size::new(640.0, 240.0)),
        (Illustration::NoApps, icons::NO_APPS_SIZE),
    ] {
        for dark in [true, false] {
            let handle = icons::illustration(which, dark);
            let mut frames = Frames::new(size);
            let art = || -> Element<'static, ()> {
                svg(handle.clone())
                    .width(size.width)
                    .height(size.height)
                    .into()
            };
            let theme = cosmic::Theme::dark();
            let on_black = frames.drawn_over(art(), &theme, Color::BLACK);
            let on_white = frames.drawn_over(art(), &theme, Color::WHITE);
            assert!(
                on_black == on_white,
                "{which:?}, dark={dark}: some of it lets the window through"
            );
        }
    }
}

#[test]
fn the_about_heros_sky_fades_in_gently_from_behind_the_list() {
    if !in_scratch_home("the_about_heros_sky_fades_in_gently_from_behind_the_list") {
        return;
    }
    // Tall enough that the page does not scroll, the last much taller than the hero.
    for size in [OPENING, Size::new(800.0, 740.0), Size::new(800.0, 1100.0)] {
        let mut scene = Scene::sized(size)
            .after([Message::Go(Page::About)])
            .resized();
        for dark in [true, false] {
            scene.window.retheme(dark);
            let theme = if dark {
                cosmic::Theme::dark()
            } else {
                cosmic::Theme::light()
            };
            let colors = scene.world.colors.borrow();
            let probe = scene.frames.probe(scene.window.view(true, &colors));
            let rows: Vec<Rectangle> = probe
                .stops
                .iter()
                .filter(|stop| stop.width > size.width / 2.0)
                .copied()
                .collect();
            let first = *rows.first().expect("the list's rows");
            let last = *rows.last().expect("the list's rows");
            let drawn = scene.frames.drawn(scene.window.view(true, &colors), &theme);
            drop(colors);
            let hero = scene.hero(dark).bounds;
            let what = format!("a window of {size:?}, dark={dark}");

            // Behind the list the sky has begun: in the rows' padding, the last is tinted against the first.
            let padding = first.x + 6.0;
            let high = drawn.at(Point::new(padding, first.y + 4.0));
            let low = drawn.at(Point::new(padding, last.y + last.height - 4.0));
            assert!(
                channel_gap(high, low) > TINTED,
                "{what}: behind the list the sky has not begun ({high:?} above, {low:?} below)"
            );

            // Below the list, the fade joins the hero's sky without a seam all along its top.
            if hero.y > last.y + last.height + 2.0 {
                for tenth in 0..10u8 {
                    let x = (size.width * (f32::from(tenth) + 0.5) / 10.0).floor();
                    let sky = drawn.at(Point::new(x, hero.y.ceil()));
                    let above = drawn.at(Point::new(x, hero.y.ceil() - 1.0));
                    assert!(
                        channel_gap(sky, above) <= GENTLE,
                        "{what}: at x={x} the sky ends at the hero's top, {sky:?} below and {above:?} above"
                    );
                }
            }

            // Down a column of open sky, from the page's top into the hero, it changes gently everywhere but at the
            // list's edges.
            let x = (size.width * 0.66).floor();
            // The Version row, above the first link, is not a stop; it is as tall as the others.
            let behind_rows = first.y - first.height - 2.0..=last.y + last.height + 2.0;
            let mut above: Option<Color> = None;
            let mut y = HEADER_HEIGHT + 1.0;
            while y < hero.y.ceil() + 3.0 {
                if behind_rows.contains(&y) {
                    above = None;
                } else {
                    let seen = drawn.at(Point::new(x, y));
                    if let Some(above) = above {
                        assert!(
                            channel_gap(above, seen) <= GENTLE,
                            "{what}: the sky steps from {above:?} to {seen:?} at y={y}"
                        );
                    }
                    above = Some(seen);
                }
                y += 1.0;
            }
        }
    }
}

#[test]
fn the_web_browser_rows_title_sits_level_with_its_button() {
    if !in_scratch_home("the_web_browser_rows_title_sits_level_with_its_button") {
        return;
    }
    let mut scene = Scene::sized(OPENING).resized();
    let colors = scene.world.colors.borrow();
    let texts = scene.frames.probe(scene.window.view(true, &colors)).texts;
    let center = |wanted: &str| {
        texts.iter().find(|(text, _)| text == wanted).map_or_else(
            || panic!("{wanted:?} in {texts:?}"),
            |(_, bounds)| bounds.center_y(),
        )
    };
    let (title, button) = (center("Web browser"), center("Use Signpost"));
    assert!(
        (title - button).abs() <= ONE_PIXEL,
        "the title's middle is at {title}, the button's at {button}"
    );
}
