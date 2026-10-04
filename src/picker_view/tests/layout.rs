//! The picker laid out and driven with the software renderer: no window, no compositor.

use cosmic::iced::advanced::layout::Limits;
use cosmic::iced::advanced::renderer::{Headless, Style};
use cosmic::iced::advanced::widget::{Id, Tree};
use cosmic::iced::{Event, Point, Rectangle, Size, Vector, mouse};

use super::*;
use crate::frost::{Frost, scoped};
use crate::picker::{COLUMNS, Outcome};
use crate::test_support::frosted::{fill, settings, translucent};
use crate::test_support::headless::{
    self, BACKDROP, Drawn, Frames, Probe, is_rounding_apart, lies_inside, over,
};

mod border;
mod details;
mod ink;
mod menu;
mod separation;

const SCRATCH_HOME: &str = "SIGNPOST_PICKER_SCRATCH_HOME";
/// What the surface may grow to; the picker sizes itself within it.
const SURFACE: Size = Size::new(MAX_WIDTH, MAX_HEIGHT);

/// Runs test `name` again with its home in a scratch directory, and says whether this is that run.
fn in_scratch_home(name: &str) -> bool {
    headless::in_scratch_home(SCRATCH_HOME, &format!("picker_view::tests::layout::{name}"))
}

/// A picker as the app shows it: the model, what waits behind it, and the frames drawn of it.
struct Scene {
    picker: Picker,
    queue: LinkQueue,
    colors: Overrides,
    frames: Frames,
}

fn view<'a>(
    picker: &'a Picker,
    queue: &'a LinkQueue,
    colors: &'a Overrides,
    theme: &cosmic::Theme,
) -> Element<'a, Message> {
    View {
        id: window::Id::RESERVED,
        picker,
        colors,
        queue,
        dark: theme.cosmic().is_dark,
        radius: theme.cosmic().radius_m(),
    }
    .render()
}

impl Scene {
    /// The first link is the one shown; `waiting` more queue behind it.
    fn new(picker: Picker, waiting: usize) -> Self {
        let mut queue = LinkQueue::default();
        for position in 0..=waiting {
            queue.push(QueuedLink::new(format!("https://example.org/{position}")));
        }
        Self {
            picker,
            queue,
            colors: Overrides::default(),
            frames: Frames::new(SURFACE),
        }
    }

    fn probe(&mut self) -> Probe {
        let view = view(&self.picker, &self.queue, &self.colors, &dark());
        self.frames.probe(view)
    }

    /// Gives `events`, with the pointer at `at`, to the widgets, and the picker what they sent: what it
    /// made of each.
    fn send(&mut self, events: &[Event], at: Point) -> Vec<Outcome> {
        let view = view(&self.picker, &self.queue, &self.colors, &dark());
        let inputs: Vec<Input> = self
            .frames
            .send(view, events, at)
            .into_iter()
            .filter_map(|message| match message {
                Message::Tile(_, input) => Some(input),
                _ => None,
            })
            .collect();
        inputs
            .into_iter()
            .map(|input| self.picker.handle(input))
            .collect()
    }

    /// Moves the pointer to `at` and gives the picker what its widgets sent.
    fn point_at(&mut self, at: Point) {
        let moved = [Event::Mouse(mouse::Event::CursorMoved { position: at })];
        self.send(&moved, at);
    }
}

/// The middle of tile `index` of plain tiles, which fill rows of `COLUMNS`.
fn tile_center(index: usize) -> Point {
    let (row, column) = (to_f32(index / COLUMNS), to_f32(index % COLUMNS));
    Point::new(
        CARD_PADDING + column * (TILE_WIDTH + SECTION_GAP) + TILE_WIDTH / 2.0,
        CARD_PADDING
            + HEADER_HEIGHT
            + SECTION_GAP
            + row * (TILE_HEIGHT + SECTION_GAP)
            + TILE_HEIGHT / 2.0,
    )
}

fn plain_picker(tiles: usize) -> Picker {
    Picker::new(URI.into(), vec![plain_tile("firefox.desktop"); tiles])
}

#[test]
fn tiles_take_no_focus_stops_of_their_own() {
    if !in_scratch_home("tiles_take_no_focus_stops_of_their_own") {
        return;
    }
    let few = Scene::new(plain_picker(1), 0).probe().stops.len();
    let many = Scene::new(plain_picker(6), 0).probe().stops.len();
    assert_eq!(many, few, "six tiles add focus stops to {few}");
}

#[test]
fn the_hover_follows_the_pointer_over_tiles_however_far_it_jumps() {
    if !in_scratch_home("the_hover_follows_the_pointer_over_tiles_however_far_it_jumps") {
        return;
    }
    let mut scene = Scene::new(plain_picker(4), 0);
    let outside = Point::new(2.0, 2.0);
    let nudge = Vector::new(3.0, 0.0);
    let hover_after = |scene: &mut Scene, at: Point| {
        scene.point_at(at);
        scene.picker.hover
    };
    assert_eq!(hover_after(&mut scene, outside), None);
    assert_eq!(hover_after(&mut scene, tile_center(2)), Some(2));
    assert_eq!(
        hover_after(&mut scene, tile_center(0)),
        Some(0),
        "jumped back over two tiles"
    );
    assert_eq!(
        hover_after(&mut scene, tile_center(0) + nudge),
        Some(0),
        "the tile it left reports its exit late"
    );
    assert_eq!(hover_after(&mut scene, tile_center(3)), Some(3));
    assert_eq!(hover_after(&mut scene, outside), None);
    assert_eq!(hover_after(&mut scene, outside + nudge), None);
    assert_eq!(scene.picker.focus, 0, "the pointer never moves the focus");
}

impl Scene {
    /// The strings the picker draws while the pointer is at `at`, an open tooltip's among them.
    fn drawn_texts_with_pointer_at(&mut self, at: Point) -> Vec<String> {
        self.point_at(at);
        let view = view(&self.picker, &self.queue, &self.colors, &dark());
        self.frames.drawn_texts(view, &dark())
    }
}

#[test]
fn a_hovered_tile_draws_no_tooltip_but_a_hovered_header_button_does() {
    if !in_scratch_home("a_hovered_tile_draws_no_tooltip_but_a_hovered_header_button_does") {
        return;
    }
    let tile = Tile {
        app_name: "Google Chrome".into(),
        ..profile_tile(CHROME, "acme.example", None)
    };
    let mut scene = Scene::new(Picker::new(URI.into(), vec![tile]), 0);
    let calm = scene.drawn_texts_with_pointer_at(Point::new(2.0, 2.0));
    assert!(calm.contains(&"acme.example".to_owned()), "{calm:?}");
    let over_tile = scene.drawn_texts_with_pointer_at(tile_center(0));
    assert_eq!(scene.picker.hover, Some(0), "the pointer is on the tile");
    assert_eq!(
        over_tile, calm,
        "a hovered tile draws no more than a calm one"
    );
    let pin = scene.probe().stops[0].center();
    let over_pin = scene.drawn_texts_with_pointer_at(pin);
    assert!(
        over_pin.contains(&fl!("pin-tooltip")),
        "the pin button's tooltip is drawn: {over_pin:?}"
    );
}

/// The strings the picker draws for a lone `tile`.
fn texts_of(tile: Tile) -> Vec<String> {
    Scene::new(Picker::new(URI.into(), vec![tile]), 0)
        .probe()
        .texts
        .into_iter()
        .map(|(text, _)| text)
        .collect()
}

#[test]
fn a_profile_tile_shows_its_profile_and_not_its_app() {
    if !in_scratch_home("a_profile_tile_shows_its_profile_and_not_its_app") {
        return;
    }
    let tile = Tile {
        app_name: "Google Chrome".into(),
        ..profile_tile(CHROME, "acme.example", None)
    };
    let texts = texts_of(tile);
    assert!(texts.contains(&"acme.example".to_owned()), "{texts:?}");
    assert!(!texts.contains(&"Google Chrome".to_owned()), "{texts:?}");
}

#[test]
fn a_plain_tile_shows_its_app() {
    if !in_scratch_home("a_plain_tile_shows_its_app") {
        return;
    }
    let tile = Tile {
        app_name: "Firefox".into(),
        label: "Firefox".into(),
        ..plain_tile("firefox.desktop")
    };
    let texts = texts_of(tile);
    assert!(texts.contains(&"Firefox".to_owned()), "{texts:?}");
}

/// The height of the failure banner of `picker`, laid out as wide as the card's content.
fn banner_laid_out(picker: &Picker) -> f32 {
    let (queue, colors) = (LinkQueue::default(), Overrides::default());
    let view = View {
        id: window::Id::RESERVED,
        picker,
        colors: &colors,
        queue: &queue,
        dark: true,
        radius: cosmic::Theme::dark().cosmic().radius_m(),
    };
    let error = picker.error.as_deref().expect("a failed picker");
    let mut banner = view.banner(error);
    let width = card_width(picker) - 2.0 * CARD_PADDING;
    let limits = Limits::new(Size::ZERO, Size::new(width, f32::INFINITY));
    let mut tree = Tree::new(&banner);
    banner
        .as_widget_mut()
        .layout(&mut tree, &headless::renderer(), &limits)
        .size()
        .height
}

/// `lines` lines of error output, as a launcher prints them.
fn long_error(lines: usize) -> String {
    "/usr/lib/browser/browser --no-sandbox --profile-directory=Default\n".repeat(lines)
}

fn failed_picker(tiles: usize, target: &str, error: String) -> Picker {
    let mut picker = plain_picker(tiles);
    let target = Target {
        app_id: "firefox.desktop".into(),
        profile_key: None,
        label: target.into(),
    };
    picker.fail(error, Some(target));
    picker
}

#[test]
fn the_collapsed_banner_is_as_tall_as_the_grid_reserves_for_it() {
    if !in_scratch_home("the_collapsed_banner_is_as_tall_as_the_grid_reserves_for_it") {
        return;
    }
    let long_label = "A profile named for everything it is used for at work ".repeat(4);
    for label in ["Work", long_label.as_str()] {
        let picker = failed_picker(3, label, "exited".into());
        let reserved = banner_height(&picker).expect("a banner");
        let laid_out = banner_laid_out(&picker);
        assert!(near(laid_out, reserved), "target {label:?}: {laid_out}");
    }
}

#[test]
fn long_details_scroll_inside_the_banner_instead_of_growing_it() {
    if !in_scratch_home("long_details_scroll_inside_the_banner_instead_of_growing_it") {
        return;
    }
    let mut picker = failed_picker(3, "Work", long_error(40));
    picker.handle(Input::ToggleDetails);
    let reserved = banner_height(&picker).expect("a banner");
    let laid_out = banner_laid_out(&picker);
    assert!(near(laid_out, reserved), "{laid_out} against {reserved}");
}

#[test]
fn a_long_failure_leaves_the_grid_a_tile_row_and_the_queue_row_its_place() {
    if !in_scratch_home("a_long_failure_leaves_the_grid_a_tile_row_and_the_queue_row_its_place") {
        return;
    }
    for details_open in [false, true] {
        let mut picker = failed_picker(5, "Work", long_error(40));
        if details_open {
            picker.handle(Input::ToggleDetails);
        }
        let probe = Scene::new(picker, 2).probe();
        let card = probe.container_named(&PICKER_AUTOSIZE).expect("the card");
        let grid = probe
            .scrollable_named(&PICKER_SCROLL)
            .expect("the grid")
            .bounds;
        let queue_row = probe.stops.last().expect("the queue row is a stop");
        let note = format!("details open: {details_open}, {card:?}");
        assert!(card.height <= MAX_HEIGHT, "card {note}");
        assert!(grid.height >= TILE_HEIGHT, "grid {grid:?}, {note}");
        assert!(
            near(queue_row.height, QUEUE_ROW_HEIGHT),
            "queue row {queue_row:?}, {note}"
        );
        assert!(
            queue_row.y + queue_row.height <= card.y + card.height + 0.01,
            "the queue row is inside the card: {queue_row:?}, {note}"
        );
        assert!(grid.y + grid.height <= queue_row.y + 0.01, "{note}");
    }
}

/// The brightness separating a dark picture from a light one, on a scale from 0 (black) to 1 (white).
const MIDPOINT: f64 = 0.5;

/// The mean brightness of the rows at the bottom of the empty picker's card that only its illustration
/// fills, drawn for the theme's tone.
fn desert_brightness(dark: bool) -> f64 {
    let picker = Picker::new(URI.into(), Vec::new());
    let (queue, colors) = (LinkQueue::default(), Overrides::default());
    let view = View {
        id: window::Id::RESERVED,
        picker: &picker,
        colors: &colors,
        queue: &queue,
        dark,
        radius: cosmic::Theme::dark().cosmic().radius_m(),
    };
    let mut frames = Frames::new(SURFACE);
    let mut ui = frames.build(view.render());
    let mut probe = Probe::default();
    ui.operate(&frames.renderer, &mut probe);
    ui.draw(
        &mut frames.renderer,
        &cosmic::Theme::dark(),
        &Style::default(),
        mouse::Cursor::Unavailable,
    );
    let surface = Rectangle::new(Point::ORIGIN, SURFACE)
        .snap()
        .expect("a surface");
    let pixels =
        frames
            .renderer
            .screenshot(Size::new(surface.width, surface.height), 1.0, Color::BLACK);
    let card = probe.container_named(&PICKER_AUTOSIZE).expect("the card");
    let desert = Rectangle {
        y: card.y + card.height - NO_APPS_SCENE,
        height: NO_APPS_SCENE,
        ..card
    }
    .snap()
    .expect("a desert");
    let bytes_per_row = 4 * surface.width as usize;
    let (first, last) = (
        4 * desert.x as usize,
        4 * (desert.x + desert.width) as usize,
    );
    let channels: u32 = pixels
        .chunks_exact(bytes_per_row)
        .skip(desert.y as usize)
        .take(desert.height as usize)
        .flat_map(|row| row[first..last].as_chunks::<4>().0.iter())
        .map(|pixel| {
            pixel[..3]
                .iter()
                .map(|&channel| u32::from(channel))
                .sum::<u32>()
        })
        .sum();
    f64::from(channels) / f64::from(3 * 255 * desert.width * desert.height)
}

#[test]
fn the_no_apps_card_draws_the_desert_of_the_themes_tone() {
    if !in_scratch_home("the_no_apps_card_draws_the_desert_of_the_themes_tone") {
        return;
    }
    let (dark, light) = (desert_brightness(true), desert_brightness(false));
    assert!(
        dark < MIDPOINT,
        "a dark theme drew a desert of brightness {dark}"
    );
    assert!(
        light > MIDPOINT,
        "a light theme drew a desert of brightness {light}"
    );
}

#[test]
fn the_no_apps_card_draws_the_shared_illustration_of_the_themes_tone_in_every_build() {
    if !in_scratch_home(
        "the_no_apps_card_draws_the_shared_illustration_of_the_themes_tone_in_every_build",
    ) {
        return;
    }
    let picker = Picker::new(URI.into(), Vec::new());
    let (queue, colors) = (LinkQueue::default(), Overrides::default());
    let mut frames = Frames::new(SURFACE);
    for dark in [true, false] {
        let shared = icons::no_apps_rounded(dark, cosmic::Theme::dark().cosmic().radius_m());
        for build in 1..=2 {
            let card = View {
                id: window::Id::RESERVED,
                picker: &picker,
                colors: &colors,
                queue: &queue,
                dark,
                radius: cosmic::Theme::dark().cosmic().radius_m(),
            };
            let drawn = frames.drawn_copy_of(&shared, card.render());
            assert!(
                std::ptr::eq(drawn.data(), shared.data()),
                "build {build} with dark={dark} drew an illustration of its own"
            );
        }
    }
}

/// How opaque the translucent theme's background is; low, so what shows through it is plain.
const FROSTED_ALPHA: f32 = 0.1;

impl Scene {
    /// The surface as the picker draws itself under `theme`.
    fn drawn(&mut self, theme: &cosmic::Theme) -> Drawn {
        let view = view(&self.picker, &self.queue, &self.colors, theme);
        self.frames.drawn(view, theme)
    }
}

/// The card's gap between its header and its tiles, away from every widget.
fn card_interior() -> Point {
    Point::new(
        MIN_CARD_WIDTH / 2.0,
        CARD_PADDING + HEADER_HEIGHT + SECTION_GAP / 2.0,
    )
}

/// The card and its bands as one block, and where the card itself ends: its last row is the queue row.
fn card_and_bands(probe: &Probe) -> (Rectangle, f32) {
    let block = probe
        .container_named(&PICKER_AUTOSIZE)
        .expect("the card and its bands");
    let queue_row = probe.stops.last().expect("the queue row is a stop");
    (block, queue_row.y + queue_row.height + CARD_PADDING)
}

const SHORT_CARD: &str = "a short card";

/// Pickers whose card is short, is held to the height cap by its tiles, and is held to it by tiles under
/// open failure details.
fn pickers() -> [(&'static str, Picker); 3] {
    let mut failed = failed_picker(2 * COLUMNS, "Work", long_error(40));
    failed.handle(Input::ToggleDetails);
    [
        (SHORT_CARD, plain_picker(1)),
        ("a card of tiles at the cap", plain_picker(4 * COLUMNS)),
        ("a card with open details at the cap", failed),
    ]
}

/// The no-apps card under a failure banner, with its details closed and open: the cards whose fixed
/// content alone can outgrow the height cap.
fn empty_pickers() -> [(&'static str, Picker); 2] {
    let mut open = failed_picker(0, "Work", long_error(40));
    open.handle(Input::ToggleDetails);
    [
        (
            "an empty card with a failure",
            failed_picker(0, "Work", long_error(40)),
        ),
        ("an empty card with open details", open),
    ]
}

/// Every kind of card the bands can sit under.
fn every_picker() -> impl Iterator<Item = (&'static str, Picker)> {
    pickers().into_iter().chain(empty_pickers())
}

#[test]
fn a_translucent_card_looks_the_same_however_many_links_wait_behind_it() {
    if !in_scratch_home("a_translucent_card_looks_the_same_however_many_links_wait_behind_it") {
        return;
    }
    for dark in [true, false] {
        let theme = translucent(dark, FROSTED_ALPHA);
        for (kind, picker) in every_picker() {
            let alone = Scene::new(picker.clone(), 0)
                .drawn(&theme)
                .at(card_interior());
            for waiting in [1, 2] {
                let queued = Scene::new(picker.clone(), waiting)
                    .drawn(&theme)
                    .at(card_interior());
                assert!(
                    is_rounding_apart(queued, alone),
                    "dark={dark}, {kind}: {waiting} waiting draw the card's interior as {queued:?}, not {alone:?}"
                );
            }
        }
    }
}

#[test]
fn no_band_shows_through_the_card_however_tall_the_card_is() {
    if !in_scratch_home("no_band_shows_through_the_card_however_tall_the_card_is") {
        return;
    }
    for dark in [true, false] {
        let theme = translucent(dark, FROSTED_ALPHA);
        let card = over(fill(&theme, &theme::Container::Background), BACKDROP);
        for waiting in [1, 2] {
            for (kind, picker) in pickers() {
                let mut scene = Scene::new(picker, waiting);
                let (block, card_bottom) = card_and_bands(&scene.probe());
                let above_the_edge = Point::new(
                    block.x + block.width / 2.0,
                    card_bottom - CARD_PADDING / 2.0,
                );
                let drawn = scene.drawn(&theme).at(above_the_edge);
                assert!(
                    is_rounding_apart(drawn, card),
                    "dark={dark}, {waiting} waiting, {kind}: the card's lower interior is drawn {drawn:?}, not {card:?}"
                );
            }
        }
    }
}

#[test]
fn each_waiting_link_shows_one_band_below_the_card_with_a_single_themed_fill() {
    if !in_scratch_home("each_waiting_link_shows_one_band_below_the_card_with_a_single_themed_fill")
    {
        return;
    }
    for dark in [true, false] {
        let theme = translucent(dark, FROSTED_ALPHA);
        let themed = over(theme.cosmic().background(true).base.into(), BACKDROP);
        for waiting in [1, 2] {
            for (kind, picker) in every_picker() {
                let mut scene = Scene::new(picker, waiting);
                let (block, card_bottom) = card_and_bands(&scene.probe());
                let bands_top = block.y + block.height - SHEET_STEP * to_f32(waiting);
                let note = format!("dark={dark}, {waiting} waiting, {kind}");
                assert!(
                    card_bottom <= bands_top + 0.01,
                    "{note}: the card ends at {card_bottom}, its bands start at {bands_top}"
                );
                assert!(
                    block.height <= MAX_HEIGHT,
                    "{note}: the card and its bands are {} tall",
                    block.height
                );
                let pixels = scene.drawn(&theme);
                for band in 0..waiting {
                    let middle = Point::new(
                        block.x + block.width / 2.0,
                        card_bottom + SHEET_STEP * (to_f32(band) + 0.5),
                    );
                    let drawn = pixels.at(middle);
                    assert!(
                        is_rounding_apart(drawn, themed),
                        "{note}, band {band}: drawn {drawn:?}, one themed fill gives {themed:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn the_queue_row_stays_whole_above_the_bands_whatever_the_card_holds() {
    if !in_scratch_home("the_queue_row_stays_whole_above_the_bands_whatever_the_card_holds") {
        return;
    }
    for waiting in [1, 2] {
        for (kind, picker) in every_picker() {
            let probe = Scene::new(picker, waiting).probe();
            let (block, _) = card_and_bands(&probe);
            let queue_row = probe.stops.last().expect("the queue row is a stop");
            let bands_top = block.y + block.height - SHEET_STEP * to_f32(waiting);
            let note = format!("{waiting} waiting, {kind}: queue row {queue_row:?}");
            assert!(near(queue_row.height, QUEUE_ROW_HEIGHT), "{note}");
            assert!(
                queue_row.y + queue_row.height + CARD_PADDING <= bands_top + 0.01,
                "{note}, bands from {bands_top}"
            );
        }
    }
}

#[test]
fn a_card_at_the_cap_and_its_bands_fill_it_exactly() {
    if !in_scratch_home("a_card_at_the_cap_and_its_bands_fill_it_exactly") {
        return;
    }
    for waiting in [1, 2] {
        for (kind, picker) in every_picker().filter(|(kind, _)| *kind != SHORT_CARD) {
            let (block, card_bottom) = card_and_bands(&Scene::new(picker, waiting).probe());
            let note = format!("{waiting} waiting, {kind}");
            assert!(near(block.height, MAX_HEIGHT), "{note}: block {block:?}");
            assert!(
                near(card_bottom, MAX_HEIGHT - SHEET_STEP * to_f32(waiting)),
                "{note}: the card ends at {card_bottom}"
            );
        }
    }
}

#[test]
fn a_card_taller_than_the_cap_is_cut_to_leave_the_bands_their_room() {
    if !in_scratch_home("a_card_taller_than_the_cap_is_cut_to_leave_the_bands_their_room") {
        return;
    }
    let card_id = Id::new("a card taller than the cap");
    for waiting in [1, 2] {
        let card = container(widget::Space::new().height(2.0 * MAX_HEIGHT)).id(card_id.clone());
        let probe = Frames::new(SURFACE).probe(with_sheets(card.into(), waiting));
        let card = probe.container_named(&card_id).expect("the card");
        let room = MAX_HEIGHT - SHEET_STEP * to_f32(waiting);
        assert!(
            card.height <= room + 0.01,
            "{waiting} waiting: the card is {} tall, its room {room}",
            card.height
        );
    }
}

#[test]
fn the_desert_keeps_its_bottom_on_the_cards_bottom_edge() {
    if !in_scratch_home("the_desert_keeps_its_bottom_on_the_cards_bottom_edge") {
        return;
    }
    let shared = icons::no_apps_rounded(true, dark().cosmic().radius_m());
    for waiting in [0, 1, 2] {
        for (kind, picker) in empty_pickers() {
            let mut scene = Scene::new(picker, waiting);
            let block = scene
                .probe()
                .container_named(&PICKER_AUTOSIZE)
                .expect("the card");
            let card_bottom = block.y + block.height - SHEET_STEP * to_f32(waiting);
            let view = view(&scene.picker, &scene.queue, &scene.colors, &dark());
            let desert = scene.frames.drawn_svg_of(&shared, view);
            let note = format!(
                "{waiting} waiting, {kind}: drawn {:?}, shown {:?}",
                desert.bounds, desert.visible
            );
            assert!(
                near(desert.visible.y + desert.visible.height, card_bottom),
                "{note}"
            );
            assert!(
                near(desert.bounds.y + desert.bounds.height, card_bottom),
                "{note}"
            );
        }
    }
}

#[test]
fn open_details_keep_their_cap_in_the_empty_card() {
    if !in_scratch_home("open_details_keep_their_cap_in_the_empty_card") {
        return;
    }
    for (kind, picker) in empty_pickers()
        .into_iter()
        .filter(|(_, picker)| picker.details_open)
    {
        for waiting in [0, 1, 2] {
            let probe = Scene::new(picker.clone(), waiting).probe();
            let [details] = probe.scrollables.as_slice() else {
                panic!("{waiting} waiting, {kind}: the details are the one scrollable");
            };
            assert!(
                near(details.bounds.height, DETAILS_MAX_HEIGHT),
                "{waiting} waiting, {kind}: {:?}",
                details.bounds
            );
        }
    }
}

/// The height of the no-apps panel's words, laid out as wide as the panel.
fn no_apps_words_laid_out() -> f32 {
    let picker = Picker::new(URI.into(), Vec::new());
    let (queue, colors) = (LinkQueue::default(), Overrides::default());
    let view = View {
        id: window::Id::RESERVED,
        picker: &picker,
        colors: &colors,
        queue: &queue,
        dark: true,
        radius: cosmic::Theme::dark().cosmic().radius_m(),
    };
    let mut words = view.no_apps_words();
    let width = card_width(&picker) - 2.0 * CARD_PADDING;
    let limits = Limits::new(Size::ZERO, Size::new(width, f32::INFINITY));
    let mut tree = Tree::new(&words);
    words
        .as_widget_mut()
        .layout(&mut tree, &headless::renderer(), &limits)
        .size()
        .height
}

#[test]
fn the_no_apps_panel_is_never_left_less_height_than_its_words_need() {
    if !in_scratch_home("the_no_apps_panel_is_never_left_less_height_than_its_words_need") {
        return;
    }
    let words = no_apps_words_laid_out();
    for (kind, picker) in empty_pickers() {
        for waiting in 0..=2 * MAX_SHEETS {
            let (panel, _) = no_apps_heights(&picker, waiting);
            assert!(
                panel >= words,
                "{waiting} waiting, {kind}: a panel of {panel} for words of {words}"
            );
        }
    }
}

/// Inside the header's badge, clear of its icon.
fn badge_interior() -> Point {
    Point::new(CARD_PADDING + 3.0, CARD_PADDING + HEADER_BADGE_SIZE / 2.0)
}

/// Inside the lower left of the second tile, clear of its icon and label.
fn tile_interior() -> Point {
    tile_center(1) + Vector::new(10.0 - TILE_WIDTH / 2.0, TILE_HEIGHT / 2.0 - 10.0)
}

#[test]
fn the_pickers_root_badge_and_tiles_are_translucent_by_the_system_interface_setting() {
    if !in_scratch_home(
        "the_pickers_root_badge_and_tiles_are_translucent_by_the_system_interface_setting",
    ) {
        return;
    }
    for dark in [true, false] {
        let theme = translucent(dark, FROSTED_ALPHA);
        for (windows, system_interface) in
            [(false, false), (false, true), (true, false), (true, true)]
        {
            let frost =
                Frost::default()
                    .supported()
                    .following(&settings(windows, system_interface, false));
            let mut scene = Scene::new(plain_picker(2), 0);
            let picker = view(&scene.picker, &scene.queue, &scene.colors, &theme);
            let drawn = scene.frames.drawn(
                scoped(theme.clone(), frost.picker(), picker),
                &cosmic::Theme::dark(),
            );

            let mut scope = theme.clone();
            scope.transparent = system_interface;
            let root = over(fill(&scope, &theme::Container::Background), BACKDROP);
            let raised = over(card_surface(&scope), root);
            for (part, point, expected) in [
                ("root", card_interior(), root),
                ("badge", badge_interior(), raised),
                ("tile", tile_interior(), raised),
            ] {
                let seen = drawn.at(point);
                assert!(
                    is_rounding_apart(seen, expected),
                    "dark={dark}, windows={windows}, system interface={system_interface}: the {part} is drawn {seen:?}, not {expected:?}"
                );
            }
        }
    }
}

#[test]
fn the_actions_menu_stays_opaque_in_a_translucent_picker() {
    for dark in [true, false] {
        let theme = translucent(dark, FROSTED_ALPHA);
        let menu = fill(&theme, &theme::Container::Dialog(true));
        assert!(near(menu.a, 1.0), "dark={dark}: {menu:?}");
    }
}

/// The theme the focus tests draw under.
fn dark() -> cosmic::Theme {
    cosmic::Theme::dark()
}

impl Scene {
    /// Whether tile `index` is drawn with its focus ring: the accent just inside its left edge.
    fn rings(&mut self, index: usize) -> bool {
        let edge = tile_center(index) + Vector::new(1.0 - TILE_WIDTH / 2.0, 0.0);
        let accent: Color = dark().cosmic().accent.base.into();
        is_rounding_apart(self.drawn(&dark()).at(edge), accent)
    }

    /// Whether the first row of the open menu is drawn in the accent that marks the focused row, rather than
    /// in the menu's own surface.
    fn accents_first_row(&self) -> bool {
        let theme = dark();
        let menu::Popup { size, probe } = menu::fitted(&self.picker, &self.queue, &self.colors);
        let first = probe.stops[0];
        let row = Point::new(first.x + 2.0, first.center().y);
        let surface = fill(&theme, &theme::Container::Dialog(true));
        let accented = over(accent_tint(&theme), surface);
        let popup = View {
            id: window::Id::RESERVED,
            picker: &self.picker,
            colors: &self.colors,
            queue: &self.queue,
            dark: true,
            radius: theme.cosmic().radius_m(),
        }
        .menu_popup();
        let drawn = Frames::new(size).drawn(popup, &theme).at(row);
        assert!(
            is_rounding_apart(drawn, accented) || is_rounding_apart(drawn, surface),
            "the first row is drawn {drawn:?}, in neither {accented:?} nor {surface:?}"
        );
        is_rounding_apart(drawn, accented)
    }
}

#[test]
fn a_fresh_picker_draws_no_ring_until_the_keyboard_moves_the_focus() {
    if !in_scratch_home("a_fresh_picker_draws_no_ring_until_the_keyboard_moves_the_focus") {
        return;
    }
    let mut scene = Scene::new(plain_picker(3), 0);
    assert!(!scene.rings(0), "a fresh picker has focus but shows none");
    scene.picker.handle(Input::Move(1, 0));
    assert!(scene.rings(1), "an arrow puts the ring on the second tile");
    assert!(!scene.rings(0));
}

#[test]
fn a_press_takes_the_ring_away_and_the_pointer_over_a_tile_leaves_it() {
    if !in_scratch_home("a_press_takes_the_ring_away_and_the_pointer_over_a_tile_leaves_it") {
        return;
    }
    let mut scene = Scene::new(plain_picker(3), 0);
    scene.picker.handle(Input::Move(1, 0));
    scene.point_at(tile_center(2));
    assert_eq!(scene.picker.hover, Some(2));
    assert!(
        scene.rings(1),
        "hover leaves the ring where the keyboard put it"
    );
    assert!(!scene.rings(2), "and gives the hovered tile none");

    scene.picker.handle(Input::PointerDown);
    assert!(!scene.rings(1), "a press hides the ring");
    assert_eq!(scene.picker.focus, 1, "without forgetting its tile");
    scene.picker.handle(Input::Move(1, 0));
    assert!(scene.rings(2), "the next arrow shows it again, one tile on");
}

#[test]
fn a_menu_the_pointer_opens_has_no_accented_row_and_the_menu_key_accents_the_first() {
    if !in_scratch_home(
        "a_menu_the_pointer_opens_has_no_accented_row_and_the_menu_key_accents_the_first",
    ) {
        return;
    }
    let mut keyboard = Scene::new(plain_picker(3), 0);
    keyboard.picker.handle(Input::OpenMenu(None));
    assert!(keyboard.accents_first_row(), "the Menu key");

    let at = tile_center(1);
    let mut right_click = Scene::new(plain_picker(3), 0);
    right_click.picker.handle(Input::Move(1, 0));
    let press = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right));
    right_click.send(&[press], at);
    assert_eq!(right_click.picker.menu, Some(1));
    assert!(!right_click.accents_first_row(), "a right-click");

    let mut long_press = Scene::new(plain_picker(3), 0);
    long_press.picker.handle(Input::Move(1, 0));
    let press = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
    let [Outcome::ArmLongPress(seq)] = long_press.send(&[press], at)[..] else {
        panic!("a press on a tile arms the long-press");
    };
    long_press.picker.handle(Input::LongPress(seq));
    assert_eq!(long_press.picker.menu, Some(1));
    assert!(!long_press.accents_first_row(), "a long-press");

    long_press.picker.handle(Input::Move(0, 1));
    assert!(
        !long_press.accents_first_row(),
        "an arrow shows the row it moved to, not the first"
    );
}
