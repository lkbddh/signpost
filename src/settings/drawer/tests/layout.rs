//! The drawer laid out and driven with the software renderer: no window, no compositor.

use cosmic::iced::{Event, Point, Rectangle, Size, mouse};

use super::*;
use crate::test_support::headless::{self, Frames};

const SCRATCH_HOME: &str = "SIGNPOST_DRAWER_SCRATCH_HOME";
/// The settings window's default and smallest sizes, which the drawer is laid out in.
const WINDOW: Size = Size::new(640.0, 600.0);
const SMALLEST_WINDOW: Size = Size::new(360.0, 400.0);
/// The focus stops that are not swatches or palette colors: the drawer's Close button.
const CHROME_STOPS: usize = 1;
/// The palette's colors and Reset, which come last once a popover is open.
const POPUP_STOPS: usize = PALETTE.len() + 1;

/// Runs test `name` again with its home in a scratch directory, and says whether this is that run.
fn in_scratch_home(name: &str) -> bool {
    headless::in_scratch_home(
        SCRATCH_HOME,
        &format!("settings::drawer::tests::layout::{name}"),
    )
}

/// `drawer` over an empty page.
fn over_empty_page<'a>(app: &'a AppRow, drawer: &'a Drawer) -> Element<'a, Message> {
    let page = widget::Space::new()
        .width(Length::Fill)
        .height(Length::Fill);
    view(page.into(), drawer, app, &Overrides::default())
}

struct Scene {
    app: AppRow,
    drawer: Drawer,
    frames: Frames,
}

impl Scene {
    fn new(popover: Option<&str>, surface: Size) -> Self {
        let mut drawer = Drawer::open(CHROME.to_owned());
        drawer.swatch = popover.map(str::to_owned);
        Self {
            app: two_profiles(),
            drawer,
            frames: Frames::new(surface),
        }
    }

    fn stops(&mut self) -> Vec<Rectangle> {
        self.frames
            .probe(over_empty_page(&self.app, &self.drawer))
            .stops
    }

    /// Presses and releases the left button at `at`, and returns what the swatches sent.
    fn click(&mut self, at: Point) -> Vec<Swatch> {
        let events = [
            Event::Mouse(mouse::Event::CursorMoved { position: at }),
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ];
        self.frames
            .send(over_empty_page(&self.app, &self.drawer), &events, at)
            .into_iter()
            .filter_map(|message| match message {
                Message::Swatch(swatch) => Some(swatch),
                _ => None,
            })
            .collect()
    }
}

fn center(bounds: Rectangle) -> Point {
    bounds.center()
}

#[test]
fn pressing_a_swatch_opens_its_popover_with_every_color_and_reset_in_it() {
    if !in_scratch_home("pressing_a_swatch_opens_its_popover_with_every_color_and_reset_in_it") {
        return;
    }
    let mut closed = Scene::new(None, WINDOW);
    let stops = closed.stops();
    assert_eq!(stops.len(), CHROME_STOPS + 2, "Close and two swatches");
    assert_eq!(
        closed.click(center(stops[CHROME_STOPS + 1])),
        [Swatch::Open("Profile 1".to_owned())]
    );

    for (surface, profile) in [
        (WINDOW, "Default"),
        (WINDOW, "Profile 1"),
        (SMALLEST_WINDOW, "Default"),
        (SMALLEST_WINDOW, "Profile 1"),
    ] {
        let mut open = Scene::new(Some(profile), surface);
        let stops = open.stops();
        assert_eq!(stops.len(), CHROME_STOPS + 2 + POPUP_STOPS);
        let window = Rectangle::with_size(surface);
        for stop in &stops {
            let corner = Point::new(stop.x + stop.width, stop.y + stop.height);
            let note = format!("{profile} in {surface:?}: {stop:?}");
            assert!(window.contains(stop.position()), "starts inside {note}");
            assert!(window.contains(corner), "ends inside {note}");
        }
    }
}

#[test]
fn choosing_a_color_or_reset_sends_only_that_choice_and_the_popup_padding_closes_nothing() {
    if !in_scratch_home(
        "choosing_a_color_or_reset_sends_only_that_choice_and_the_popup_padding_closes_nothing",
    ) {
        return;
    }
    let mut scene = Scene::new(Some("Default"), WINDOW);
    let stops = scene.stops();
    let popup = stops[stops.len() - POPUP_STOPS..].to_vec();
    let (colors, reset) = popup.split_at(PALETTE.len());

    assert_eq!(
        scene.click(center(colors[9])),
        [Swatch::Choose(Some(PALETTE[9]))]
    );
    assert_eq!(scene.click(center(reset[0])), [Swatch::Choose(None)]);

    let beside_first_color = Point::new(colors[0].x - 4.0, colors[0].y + 4.0);
    assert_eq!(scene.click(beside_first_color), [], "padding of the popup");
}

#[test]
fn pressing_outside_the_popover_closes_it_and_pressing_another_swatch_leaves_that_one_open() {
    if !in_scratch_home(
        "pressing_outside_the_popover_closes_it_and_pressing_another_swatch_leaves_that_one_open",
    ) {
        return;
    }
    let mut scene = Scene::new(Some("Profile 1"), WINDOW);
    assert_eq!(
        scene.click(Point::new(8.0, 8.0)),
        [Swatch::Close("Profile 1".to_owned())],
        "the page beside the drawer"
    );

    // The popup covers the swatches below its own, so the one pressed is the profile above.
    let above = center(scene.stops()[CHROME_STOPS]);
    let sent = scene.click(above);
    assert_eq!(
        sent,
        [
            Swatch::Close("Profile 1".to_owned()),
            Swatch::Open("Default".to_owned())
        ]
    );
    for order in [sent.clone(), sent.into_iter().rev().collect()] {
        let mut drawer = Drawer::open(CHROME.to_owned());
        drawer.swatch = Some("Profile 1".to_owned());
        let mut overrides = Overrides::default();
        for swatch in order {
            drawer.update(swatch, None, &mut overrides);
        }
        assert_eq!(drawer.swatch.as_deref(), Some("Default"));
    }
}
