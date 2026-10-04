//! The actions menu in a popup of its own: the card it leaves as it was, the surface it draws and the
//! presses it takes.

use cosmic::iced::widget::container::Catalog;
use cosmic::iced::{Shadow, touch};

use super::*;
use crate::test_support::headless::NEW_POPUP;

/// What the card's own layout holds still: its autosize bounds and where every tile is.
#[derive(Debug, PartialEq)]
struct Laid {
    card: Rectangle,
    tiles: Vec<Rectangle>,
}

fn in_scratch_home(name: &str) -> bool {
    headless::in_scratch_home(
        SCRATCH_HOME,
        &format!("picker_view::tests::layout::menu::{name}"),
    )
}

/// A plain tile that offers its private window and `desktop` more actions.
fn tile_with_actions(desktop: usize) -> Tile {
    let mut actions = vec![TileAction::Private];
    actions.extend((0..desktop).map(|number| TileAction::Desktop {
        id: format!("action-{number}"),
        label: format!("Action {number}"),
    }));
    Tile {
        actions,
        ..plain_tile("firefox.desktop")
    }
}

/// A plain tile that offers one desktop action, labelled `label`.
fn tile_with_label(label: &str) -> Tile {
    Tile {
        actions: vec![TileAction::Desktop {
            id: "labelled".into(),
            label: label.into(),
        }],
        ..plain_tile("firefox.desktop")
    }
}

/// An action label far longer than the labels of the other rows.
const LONG_LABEL: &str = "Open a new window in a temporary profile with every extension turned off";

const EIGHT_ROWS: usize = 4;

/// A picker, the tile whose menu opens and how many links wait behind it, for each way the card around the
/// menu can be built.
fn cards() -> Vec<(&'static str, Picker, usize, usize)> {
    let mut eight = plain_picker(5);
    eight.tiles[4] = tile_with_actions(EIGHT_ROWS);
    let mut open_details = failed_picker(5, "Work", long_error(40));
    open_details.handle(Input::ToggleDetails);
    vec![
        ("one tile", plain_picker(1), 0, 0),
        ("a tile in a lower row", plain_picker(9), 8, 0),
        ("a menu of eight rows", eight, 4, 0),
        (
            "a failure banner",
            failed_picker(5, "Work", "exited".into()),
            4,
            0,
        ),
        ("open failure details", open_details, 4, 0),
        ("links waiting", plain_picker(5), 4, 2),
        (
            "a banner and links waiting",
            failed_picker(5, "Work", "exited".into()),
            4,
            2,
        ),
    ]
}

fn laid_out(scene: &mut Scene) -> Laid {
    let probe = scene.probe();
    Laid {
        card: probe.container_named(&PICKER_AUTOSIZE).expect("the card"),
        tiles: (0..scene.picker.tiles.len())
            .map(|index| probe.container_named(&tile_id(index)).expect("a tile"))
            .collect(),
    }
}

#[test]
fn opening_and_closing_the_menu_keeps_the_card_and_every_tile_where_they_are() {
    if !in_scratch_home("opening_and_closing_the_menu_keeps_the_card_and_every_tile_where_they_are")
    {
        return;
    }
    for (kind, picker, tile, waiting) in cards() {
        let mut scene = Scene::new(picker, waiting);
        let closed = laid_out(&mut scene);
        scene.picker.handle(Input::OpenMenu(Some(tile)));
        assert_eq!(scene.picker.menu, Some(tile), "{kind}");
        assert_eq!(laid_out(&mut scene), closed, "{kind}: the menu is open");
        assert!(
            scene.probe().scrollable_named(&MENU_SCROLL).is_none(),
            "{kind}: the menu is not in the card"
        );
        scene.picker.handle(Input::CloseMenu);
        assert_eq!(
            laid_out(&mut scene),
            closed,
            "{kind}: the menu closed again"
        );
    }
}

/// A surface in which nothing the menu holds is held back.
const OPEN_SURFACE: Size = Size::new(4096.0, 4096.0);

/// What a popup shows once it has the surface it asked for.
pub(super) struct Popup {
    pub(super) size: Size,
    pub(super) probe: Probe,
}

/// The popup of `picker`'s open menu as the runtime sizes it: it starts as a surface of [`NEW_POPUP`], asks
/// for the size its widgets need, and is then shown in a surface of that size.
pub(super) fn fitted(picker: &Picker, queue: &LinkQueue, colors: &Overrides) -> Popup {
    let size = Frames::new(NEW_POPUP)
        .requested_size(popup_of(picker, queue, colors))
        .expect("the popup asks to be resized");
    let probe = Frames::new(size).probe(popup_of(picker, queue, colors));
    Popup { size, probe }
}

/// The menus the popup is shown for, from the shortest to one with a label far longer than the rest.
fn menus() -> [(&'static str, Picker); 3] {
    [
        (
            "the shortest menu",
            open_menu_of(plain_tile("firefox.desktop")),
        ),
        ("eight rows", open_menu_of(tile_with_actions(EIGHT_ROWS))),
        ("a long label", open_menu_of(tile_with_label(LONG_LABEL))),
    ]
}

#[test]
fn a_new_popup_asks_for_a_surface_in_whole_pixels_that_holds_every_row() {
    if !in_scratch_home("a_new_popup_asks_for_a_surface_in_whole_pixels_that_holds_every_row") {
        return;
    }
    let (queue, colors) = (LinkQueue::default(), Overrides::default());
    for (kind, picker) in menus() {
        let mut frames = Frames::new(NEW_POPUP);
        let asked = frames
            .requested_size(popup_of(&picker, &queue, &colors))
            .unwrap_or_else(|| panic!("{kind}: the popup asks for no size"));
        assert_eq!(
            asked,
            Size::new(asked.width.round(), asked.height.round()),
            "{kind}: the runtime rounds the size it is asked for to whole pixels"
        );
        let rows = frames.probe(popup_of(&picker, &queue, &colors)).stops;
        assert_eq!(rows.len(), picker.menu_items().len(), "{kind}");
        for (row, bounds) in rows.iter().enumerate() {
            assert!(
                lies_inside(*bounds, Rectangle::with_size(asked)),
                "{kind}: row {row} {bounds:?} is not inside the {asked:?} the popup asked for"
            );
        }
    }
}

#[test]
fn a_view_that_never_asks_to_be_resized_is_seen_to_ask_for_nothing() {
    if !in_scratch_home("a_view_that_never_asks_to_be_resized_is_seen_to_ask_for_nothing") {
        return;
    }
    let asked = Frames::new(NEW_POPUP).requested_size::<Message>(widget::Space::new());
    assert_eq!(asked, None);
}

#[test]
fn the_popup_holds_every_row_whole_and_its_list_needs_no_scroll() {
    if !in_scratch_home("the_popup_holds_every_row_whole_and_its_list_needs_no_scroll") {
        return;
    }
    let (queue, colors) = (LinkQueue::default(), Overrides::default());
    for (kind, picker) in menus() {
        let Popup { size, probe } = fitted(&picker, &queue, &colors);
        let popup = Rectangle::with_size(size);
        assert_eq!(probe.stops.len(), picker.menu_items().len(), "{kind}");
        for (row, bounds) in probe.stops.iter().enumerate() {
            assert!(
                lies_inside(*bounds, popup),
                "{kind}: row {row} {bounds:?} is not inside the popup {popup:?}"
            );
        }
        let list = probe.scrollable_named(&MENU_SCROLL).expect("the list");
        assert!(
            list.content.height <= list.bounds.height && near(list.offset.y, 0.0),
            "{kind}: the list is {} tall in a viewport {} tall",
            list.content.height,
            list.bounds.height
        );
    }
}

/// How wide `text` is on its own, where nothing holds it back.
fn width_alone(text: widget::Text<'_, cosmic::Theme, cosmic::Renderer>) -> f32 {
    Frames::new(OPEN_SURFACE)
        .probe::<Message>(text)
        .texts
        .first()
        .expect("the text")
        .1
        .width
}

#[test]
fn no_label_or_key_in_the_popup_is_clipped() {
    if !in_scratch_home("no_label_or_key_in_the_popup_is_clipped") {
        return;
    }
    let (queue, colors) = (LinkQueue::default(), Overrides::default());
    for (kind, picker) in menus() {
        let keys: Vec<&str> = picker
            .menu_items()
            .iter()
            .flat_map(|item| item.shortcut().iter().copied())
            .collect();
        let Popup { probe, .. } = fitted(&picker, &queue, &colors);
        for (text, bounds) in &probe.texts {
            let alone = if keys.contains(&text.as_str()) {
                width_alone(widget::text::monotext(text.clone()))
            } else {
                width_alone(widget::text::body(text.clone()))
            };
            assert!(
                bounds.width >= alone,
                "{kind}: {text:?} is laid out {} wide and wants {alone}",
                bounds.width
            );
            assert!(
                probe.stops.iter().any(|row| lies_inside(*bounds, *row)),
                "{kind}: {text:?} {bounds:?} is outside every row"
            );
        }
    }
}

#[test]
fn the_longest_label_has_its_two_keys_in_its_row() {
    if !in_scratch_home("the_longest_label_has_its_two_keys_in_its_row") {
        return;
    }
    let (queue, colors) = (LinkQueue::default(), Overrides::default());
    let picker = open_menu_of(plain_tile("firefox.desktop"));
    let Popup { probe, .. } = fitted(&picker, &queue, &colors);
    let (_, label) = probe
        .texts
        .iter()
        .find(|(text, _)| text == "Open and keep picker")
        .expect("the longest label");
    let row = probe
        .stops
        .iter()
        .find(|row| lies_inside(*label, **row))
        .expect("its row");
    let keys: Vec<&str> = probe
        .texts
        .iter()
        .filter(|(_, bounds)| lies_inside(*bounds, *row) && bounds.x >= label.x + label.width)
        .map(|(text, _)| text.as_str())
        .collect();
    assert_eq!(keys, ["Ctrl", "Enter"], "after the label, in its row");
}

#[test]
fn every_row_is_as_wide_as_the_widest_and_the_popup_as_wide_as_the_rows() {
    if !in_scratch_home("every_row_is_as_wide_as_the_widest_and_the_popup_as_wide_as_the_rows") {
        return;
    }
    let (queue, colors) = (LinkQueue::default(), Overrides::default());
    for (kind, picker) in menus() {
        let Popup { size, probe } = fitted(&picker, &queue, &colors);
        let first = probe.stops[0];
        for (row, bounds) in probe.stops.iter().enumerate() {
            assert!(
                near(bounds.width, first.width) && near(bounds.x, MENU_PADDING),
                "{kind}: row {row} {bounds:?} is not as wide as the first {first:?}"
            );
        }
        assert!(
            (size.width - first.width - 2.0 * MENU_PADDING).abs() < 1.0,
            "{kind}: a popup {} wide round rows {} wide",
            size.width,
            first.width
        );
    }
}

/// Labels of one, two and three lines, which a desktop entry can name an action with.
const MIXED_LABELS: [&str; 3] = ["Short", "Two\nlines", "Three\nlines\nin all"];

#[test]
fn every_row_is_as_tall_as_the_tallest_with_its_label_in_the_middle() {
    if !in_scratch_home("every_row_is_as_tall_as_the_tallest_with_its_label_in_the_middle") {
        return;
    }
    let (queue, colors) = (LinkQueue::default(), Overrides::default());
    let tile = Tile {
        actions: MIXED_LABELS
            .map(|label| TileAction::Desktop {
                id: label.into(),
                label: label.into(),
            })
            .to_vec(),
        ..plain_tile("firefox.desktop")
    };
    let Popup { probe, .. } = fitted(&open_menu_of(tile), &queue, &colors);
    let one_line = fitted(
        &open_menu_of(plain_tile("firefox.desktop")),
        &queue,
        &colors,
    )
    .probe
    .stops[0]
        .height;
    let first = probe.stops[0];
    assert!(
        first.height > one_line,
        "{first:?} is no taller than {one_line}"
    );
    for (row, bounds) in probe.stops.iter().enumerate() {
        assert!(near(bounds.height, first.height), "row {row} {bounds:?}");
        let (text, label) = probe
            .texts
            .iter()
            .find(|(_, label)| lies_inside(*label, *bounds))
            .expect("its label");
        assert!(
            (label.center().y - bounds.center().y).abs() < 0.5,
            "row {row}: {text:?} {label:?} is not in the middle of {bounds:?}"
        );
    }
}

#[test]
fn a_longer_label_makes_the_popup_wider() {
    if !in_scratch_home("a_longer_label_makes_the_popup_wider") {
        return;
    }
    let (queue, colors) = (LinkQueue::default(), Overrides::default());
    let width_with = |label: &str| {
        let picker = open_menu_of(tile_with_label(label));
        fitted(&picker, &queue, &colors).size.width
    };
    assert!(width_with(LONG_LABEL) > width_with("New Window"));
}

#[test]
fn a_menu_taller_than_the_cap_stops_at_it_and_scrolls() {
    if !in_scratch_home("a_menu_taller_than_the_cap_stops_at_it_and_scrolls") {
        return;
    }
    let (queue, colors) = (LinkQueue::default(), Overrides::default());
    let mut many = plain_tile("firefox.desktop");
    many.actions = vec![TileAction::Private; 40];
    let Popup { size, probe } = fitted(&open_menu_of(many), &queue, &colors);
    assert!(near(size.height, MAX_HEIGHT), "{size:?}");
    let list = probe.scrollable_named(&MENU_SCROLL).expect("the list");
    assert!(list.content.height > list.bounds.height);
}

#[test]
fn a_menu_of_eight_rows_is_taller_than_the_card_of_one_tile_which_is_why_it_has_a_popup() {
    if !in_scratch_home(
        "a_menu_of_eight_rows_is_taller_than_the_card_of_one_tile_which_is_why_it_has_a_popup",
    ) {
        return;
    }
    let (queue, colors) = (LinkQueue::default(), Overrides::default());
    let menu = fitted(
        &open_menu_of(tile_with_actions(EIGHT_ROWS)),
        &queue,
        &colors,
    );
    let card = Scene::new(plain_picker(1), 0)
        .probe()
        .container_named(&PICKER_AUTOSIZE)
        .expect("the card");
    assert!(
        menu.size.height > card.height,
        "{:?} against {card:?}",
        menu.size
    );
}

fn left_press() -> Event {
    Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
}

fn left_release() -> Event {
    Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
}

fn inputs(messages: Vec<Message>) -> Vec<Input> {
    messages
        .into_iter()
        .filter_map(|message| match message {
            Message::Tile(_, input) => Some(input),
            _ => None,
        })
        .collect()
}

impl Scene {
    /// What the card's widgets send for a click of the left button at `at`.
    fn clicked(&mut self, at: Point) -> Vec<Input> {
        let view = view(&self.picker, &self.queue, &self.colors, &dark());
        inputs(self.frames.send(view, &[left_press(), left_release()], at))
    }
}

/// How a button or a finger goes down and comes up again at a point.
type Gesture = (&'static str, fn(Point) -> Event, fn(Point) -> Event);

#[test]
fn any_press_outside_the_menu_closes_it_and_does_nothing_else() {
    if !in_scratch_home("any_press_outside_the_menu_closes_it_and_does_nothing_else") {
        return;
    }
    let mut scene = Scene::new(plain_picker(3), 0);
    let stops = scene.probe().stops;
    assert_eq!(
        scene.clicked(stops[0].center()),
        [Input::TogglePin],
        "with no menu open the pin button acts"
    );
    let places = [
        ("the pin button", stops[0].center()),
        ("the copy button", stops[1].center()),
        ("the tile the menu is of", tile_center(0)),
        ("another tile", tile_center(1)),
        ("the card between its sections", card_interior()),
    ];
    let gestures: [Gesture; 4] = [
        ("the left button", |_| left_press(), |_| left_release()),
        (
            "the right button",
            |_| Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)),
            |_| Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Right)),
        ),
        (
            "the middle button",
            |_| Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Middle)),
            |_| Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Middle)),
        ),
        (
            "a touch",
            |position| {
                Event::Touch(touch::Event::FingerPressed {
                    id: touch::Finger(0),
                    position,
                })
            },
            |position| {
                Event::Touch(touch::Event::FingerLifted {
                    id: touch::Finger(0),
                    position,
                })
            },
        ),
    ];
    for (place, at) in places {
        for (how, press, release) in gestures {
            scene.picker.handle(Input::OpenMenu(Some(0)));
            let view = view(&scene.picker, &scene.queue, &scene.colors, &dark());
            let sent = inputs(scene.frames.send(view, &[press(at)], at));
            assert_eq!(sent, [Input::CloseMenu], "{how} pressed on {place}");
            scene.picker.handle(Input::CloseMenu);
            let outcomes = scene.send(&[release(at)], at);
            assert!(
                outcomes.iter().all(|outcome| *outcome == Outcome::None),
                "{how} released on {place}: {outcomes:?}"
            );
        }
    }
}

#[test]
fn the_menus_style_is_the_opaque_dialog_without_its_shadow() {
    let theme_of = |dark: bool| translucent(dark, FROSTED_ALPHA);
    for theme in [theme_of(true), theme_of(false), cosmic::Theme::dark()] {
        let dialog = theme.style(&theme::Container::Dialog(true));
        let menu = menu_style(&theme);
        assert_ne!(
            dialog.shadow,
            Shadow::default(),
            "the dialog has the shadow this style takes away"
        );
        assert_eq!(menu.shadow, Shadow::default());
        assert_eq!(
            menu.border, dialog.border,
            "the dialog's divider and radius_m"
        );
        assert_eq!(menu.background, dialog.background, "its opaque fill");
        assert!(near(fill(&theme, &theme::Container::Dialog(true)).a, 1.0));
        assert_eq!(
            (menu.text_color, menu.icon_color),
            (dialog.text_color, dialog.icon_color)
        );
    }
}

fn open_menu_of(tile: Tile) -> Picker {
    let mut picker = Picker::new(URI.into(), vec![tile]);
    picker.handle(Input::OpenMenu(None));
    picker
}

fn popup_of<'a>(
    picker: &'a Picker,
    queue: &'a LinkQueue,
    colors: &'a Overrides,
) -> Element<'a, Message> {
    View {
        id: window::Id::RESERVED,
        picker,
        colors,
        queue,
        dark: true,
        radius: dark().cosmic().radius_m(),
    }
    .menu_popup()
}

#[test]
fn the_popup_draws_the_menu_to_its_own_edges_with_no_room_for_a_shadow() {
    if !in_scratch_home("the_popup_draws_the_menu_to_its_own_edges_with_no_room_for_a_shadow") {
        return;
    }
    let (queue, colors) = (LinkQueue::default(), Overrides::default());
    for (kind, theme) in [
        ("dark", dark()),
        ("light", cosmic::Theme::light()),
        ("frosted", translucent(true, FROSTED_ALPHA)),
    ] {
        let picker = open_menu_of(tile_with_actions(EIGHT_ROWS));
        let size = fitted(&picker, &queue, &colors).size;
        assert!(size.width > 0.0 && size.height > 0.0, "{kind}: {size:?}");
        let drawn = Frames::new(size).drawn(popup_of(&picker, &queue, &colors), &theme);
        let divider: Color = theme.cosmic().primary(false).divider.into();
        let (right, bottom) = (size.width - 1.0, size.height - 1.0);
        let (middle, center) = (size.width / 2.0, size.height / 2.0);
        for (side, edge, inside) in [
            ("left", Point::new(0.0, center), Point::new(1.0, center)),
            (
                "right",
                Point::new(right, center),
                Point::new(right - 1.0, center),
            ),
            ("top", Point::new(middle, 0.0), Point::new(middle, 1.0)),
            (
                "bottom",
                Point::new(middle, bottom),
                Point::new(middle, bottom - 1.0),
            ),
        ] {
            let (seen, inner) = (drawn.at(edge), drawn.at(inside));
            assert!(
                is_rounding_apart(seen, over(divider, inner)),
                "{kind}, {side}: the edge is drawn {seen:?}, the divider over {inner:?} gives {:?}",
                over(divider, inner)
            );
        }
    }
}

#[test]
fn a_press_on_the_menus_padding_or_between_its_rows_dismisses_nothing_and_a_row_still_acts() {
    if !in_scratch_home(
        "a_press_on_the_menus_padding_or_between_its_rows_dismisses_nothing_and_a_row_still_acts",
    ) {
        return;
    }
    let (queue, colors) = (LinkQueue::default(), Overrides::default());
    let picker = open_menu_of(plain_tile("firefox.desktop"));
    let Popup { size, probe } = fitted(&picker, &queue, &colors);
    let mut frames = Frames::new(size);
    let click = |frames: &mut Frames, at: Point| {
        let view = popup_of(&picker, &queue, &colors);
        inputs(frames.send(view, &[left_press(), left_release()], at))
    };
    let middle = size.width / 2.0;
    let [first, second, third] = probe.stops[..] else {
        panic!("Open, Open and keep, Copy");
    };
    let padding = Point::new(middle, MENU_PADDING / 2.0);
    let separator = Point::new(middle, (second.y + second.height + third.y) / 2.0);
    assert_eq!(click(&mut frames, padding), [], "its padding");
    assert_eq!(
        click(&mut frames, separator),
        [],
        "the separator before Copy"
    );
    assert_eq!(
        click(&mut frames, first.center()),
        [Input::ChooseItem {
            index: 0,
            item: MenuItem::Open,
            keep_open: false
        }]
    );
}
