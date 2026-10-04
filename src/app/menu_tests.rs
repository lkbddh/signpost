//! The actions menu as the app runs it: one popup of its own, anchored to its tile, whose keys and presses
//! belong to the picker that owns it, and which goes with the menu.
//!
//! Building the app reads libcosmic's configuration and the app index, so these tests run again in a child
//! process whose home is a scratch directory.

use cosmic::cctk::wayland_protocols::xdg::shell::client::xdg_positioner::{Anchor, Gravity};
use cosmic::iced::event::Status;
use cosmic::iced::keyboard::key::{Code, Named, Physical};
use cosmic::iced::keyboard::{Key, Location, Modifiers};
use cosmic::iced::{Point, Rectangle, Size};

use super::focus_tests::{
    Seen, Shown, TALL, key_pressed, left_press, near, plain_tile, press, world_with_picker,
};
use super::*;
use crate::picker::MenuItem;
use crate::picker_view::MENU_GAP;
use crate::profiles::TileAction;
use crate::test_support::frosted::translucent;
use crate::test_support::headless::{self, lies_inside};

const SCRATCH_HOME: &str = "SIGNPOST_APP_MENU_SCRATCH_HOME";
/// How opaque the translucent theme's background is; low, so what shows through it is plain.
const FROSTED_ALPHA: f32 = 0.1;
/// The anchor rectangle is whole pixels, so it may sit this far from the tile's own bounds.
const ROUNDING: f64 = 0.5;

/// Runs test `name` again with its home in a scratch directory, and says whether this is that run.
fn in_scratch_home(name: &str) -> bool {
    headless::in_scratch_home(SCRATCH_HOME, &format!("app::menu_tests::{name}"))
}

type Check = fn(&Shown<'_>);
type Close = fn(window::Id) -> Message;

/// The user opens the menu of tile `tile` with the pointer, and the popup that shows it.
fn open_menu(shown: &mut Shown<'_>, tile: usize) -> window::Id {
    shown.input(Input::OpenMenu(Some(tile)));
    shown.popup_id()
}

fn control_key(key: &str) -> Event {
    let key = Key::Character(key.into());
    Event::Keyboard(keyboard::Event::KeyPressed {
        key: key.clone(),
        modified_key: key,
        physical_key: Physical::Code(Code::KeyC),
        location: Location::Standard,
        modifiers: Modifiers::CTRL,
        text: None,
        repeat: false,
    })
}

fn pending(shown: &Shown<'_>) -> Pending {
    shown
        .world
        .app
        .pickers
        .get(&shown.id)
        .and_then(|state| state.pending.clone())
        .expect("a launch is pending")
}

fn is_open(shown: &Shown<'_>) -> bool {
    shown.world.app.pickers.contains_key(&shown.id)
}

/// The popups the runtime was asked for and has not been asked to destroy.
fn alive(shown: &Shown<'_>) -> Vec<window::Id> {
    let mut alive = Vec::new();
    for seen in &shown.seen {
        match seen {
            Seen::Opened(popup) => alive.push(*popup),
            Seen::Destroyed(popup) => alive.retain(|kept| kept != popup),
            Seen::LayerDestroyed(_) | Seen::TokenRequested(_) => {}
        }
    }
    alive
}

#[test]
fn keys_addressed_to_the_popup_act_on_its_picker() {
    if !in_scratch_home("keys_addressed_to_the_popup_act_on_its_picker") {
        return;
    }
    let cases: [(&str, Event, Check); 5] = [
        ("Down", press(Named::ArrowDown), |shown| {
            assert_eq!(shown.picker().menu_focus, 1);
            assert!(shown.has_popup(), "the menu stays open");
        }),
        ("Enter", press(Named::Enter), |shown| {
            let attempt = pending(shown);
            assert_eq!((attempt.index, attempt.keep_open), (0, false));
            assert_eq!(shown.picker().menu, None, "the row chosen closes the menu");
            assert!(shown.has_popup(), "its popup waits for the launch's token");
        }),
        (
            "Ctrl+Enter",
            key_pressed(Named::Enter, Modifiers::CTRL),
            |shown| {
                assert!(pending(shown).keep_open, "Open and keep");
                assert_eq!(shown.picker().menu, None);
                assert!(shown.has_popup());
            },
        ),
        ("Ctrl+C", control_key("c"), |shown| {
            assert!(shown.picker().copied.is_some(), "the link was copied");
            assert!(!shown.has_popup());
        }),
        ("Esc", press(Named::Escape), |shown| {
            assert_eq!(shown.picker().menu, None);
            assert!(is_open(shown), "the picker stays");
            assert!(!shown.has_popup());
        }),
    ];
    for (_, event, check) in cases {
        let (mut world, id) = world_with_picker(3);
        let mut shown = Shown::new(&mut world, id);
        open_menu(&mut shown, 0);
        shown.press_in_popup(event);
        check(&shown);
    }
}

#[test]
fn a_second_escape_closes_the_picker_after_the_first_closed_the_menu() {
    if !in_scratch_home("a_second_escape_closes_the_picker_after_the_first_closed_the_menu") {
        return;
    }
    let (mut world, id) = world_with_picker(3);
    let mut shown = Shown::new(&mut world, id);
    open_menu(&mut shown, 0);
    shown.press_in_popup(press(Named::Escape));
    assert!(is_open(&shown) && !shown.has_popup(), "the menu went first");
    shown.press_in_picker(press(Named::Escape));
    assert!(!is_open(&shown), "then the picker");
}

#[test]
fn a_press_in_the_popup_hides_the_focus_of_its_picker() {
    if !in_scratch_home("a_press_in_the_popup_hides_the_focus_of_its_picker") {
        return;
    }
    let (mut world, id) = world_with_picker(3);
    let mut shown = Shown::new(&mut world, id);
    open_menu(&mut shown, 0);
    shown.press_in_popup(press(Named::ArrowDown));
    assert!(shown.picker().focus_visible, "the arrow showed the row");
    shown.receive_in_popup(left_press());
    assert!(!shown.picker().focus_visible, "the press took it away");
    assert_eq!(shown.picker().menu_focus, 1, "and left it where it was");
}

#[test]
fn the_popup_closing_clears_the_menu_once_and_keeps_the_picker_and_the_queue() {
    if !in_scratch_home("the_popup_closing_clears_the_menu_once_and_keeps_the_picker_and_the_queue")
    {
        return;
    }
    let (mut world, id) = world_with_picker(3);
    for link in ["https://example.org/shown", "https://example.org/waiting"] {
        world.app.queue.push(QueuedLink::new(link.to_owned()));
    }
    let mut shown = Shown::new(&mut world, id);
    let popup = open_menu(&mut shown, 1);
    for notice in ["the compositor's first", "its repeat"] {
        shown.update(Message::SurfaceClosed(popup));
        assert_eq!(shown.picker().menu, None, "{notice}");
        assert!(is_open(&shown), "{notice}: the picker stays");
        assert_eq!(
            shown.world.app.queue.waiting(),
            1,
            "{notice}: so does the queue"
        );
        assert_eq!(shown.world.app.pickers.len(), 1, "{notice}: no other opens");
        assert_eq!(shown.picker().focus, 1, "{notice}: on its tile");
    }
    assert_eq!(
        shown.seen,
        [Seen::Opened(popup)],
        "nothing is destroyed twice, and nothing else"
    );
}

#[test]
fn a_link_that_comes_while_the_picker_and_its_menu_go_waits_for_the_picker_to_be_gone() {
    if !in_scratch_home(
        "a_link_that_comes_while_the_picker_and_its_menu_go_waits_for_the_picker_to_be_gone",
    ) {
        return;
    }
    let (mut world, id) = world_with_picker(3);
    world
        .app
        .queue
        .push(QueuedLink::new("https://example.org/shown".to_owned()));
    let mut shown = Shown::new(&mut world, id);
    let popup = open_menu(&mut shown, 1);
    let closing = shown.world.app.close_picker(id);
    shown.run(closing);
    shown.world.app.backlog.add(1);
    shown.update(Message::Bus(BusEvent::Open {
        uris: vec!["https://example.org/next".to_owned()],
    }));
    assert!(
        shown.world.app.pickers.is_empty(),
        "nothing opens while the picker is still on screen"
    );
    shown.update(Message::SurfaceClosed(popup));
    assert!(
        shown.world.app.pickers.is_empty(),
        "its menu's popup going is not the picker going"
    );
    shown.update(Message::SurfaceClosed(id));
    assert_eq!(
        shown.world.app.pickers.len(),
        1,
        "the link shows once the picker is gone"
    );
    assert_eq!(shown.world.app.backlog.waiting(), 0);
}

#[test]
fn a_popup_notice_that_is_stale_leaves_the_menu_it_does_not_name() {
    if !in_scratch_home("a_popup_notice_that_is_stale_leaves_the_menu_it_does_not_name") {
        return;
    }
    let (mut world, id) = world_with_picker(3);
    let mut shown = Shown::new(&mut world, id);
    let first = open_menu(&mut shown, 0);
    let second = open_menu(&mut shown, 1);
    assert_ne!(first, second, "every menu gets a popup of its own");
    assert_eq!(
        shown.seen,
        [
            Seen::Opened(first),
            Seen::Destroyed(first),
            Seen::Opened(second)
        ]
    );
    for stale in [first, window::Id::unique()] {
        shown.update(Message::SurfaceClosed(stale));
        assert_eq!(shown.picker().menu, Some(1));
        assert_eq!(shown.popup_id(), second);
    }
    shown.update(Message::SurfaceClosed(second));
    assert_eq!(
        shown.picker().menu,
        None,
        "the notice that names it closes it"
    );
}

#[test]
fn every_way_the_menu_closes_takes_its_popup_with_it() {
    if !in_scratch_home("every_way_the_menu_closes_takes_its_popup_with_it") {
        return;
    }
    let closers = [
        Input::Escape,
        Input::CloseMenu,
        Input::PressStart(0),
        Input::Copy,
    ];
    for closer in closers {
        let (mut world, id) = world_with_picker(3);
        let mut shown = Shown::new(&mut world, id);
        let popup = open_menu(&mut shown, 0);
        shown.input(closer.clone());
        assert_eq!(shown.picker().menu, None, "{closer:?}");
        assert!(!shown.has_popup(), "{closer:?}");
        assert_eq!(
            shown.seen,
            [Seen::Opened(popup), Seen::Destroyed(popup)],
            "{closer:?}"
        );
    }
}

type Launch = fn(&mut Shown<'_>);

/// Ways to choose a row of the open menu.
fn launches() -> [(&'static str, Launch); 3] {
    [
        ("Enter", |shown| shown.press_in_popup(press(Named::Enter))),
        ("Ctrl+Enter", |shown| {
            shown.press_in_popup(key_pressed(Named::Enter, Modifiers::CTRL));
        }),
        ("a click on a row", |shown| {
            shown.input(Input::ChooseItem {
                index: 0,
                item: MenuItem::Open,
                keep_open: false,
            });
        }),
    ]
}

#[test]
fn a_launch_from_the_menu_asks_for_its_token_from_the_popup_and_the_popup_goes_after_it() {
    if !in_scratch_home(
        "a_launch_from_the_menu_asks_for_its_token_from_the_popup_and_the_popup_goes_after_it",
    ) {
        return;
    }
    let (mut world, id) = world_with_picker(3);
    let mut shown = Shown::new(&mut world, id).with_tokens();
    shown.input(Input::Click {
        index: 0,
        keep_open: false,
    });
    assert_eq!(
        shown.seen,
        [Seen::TokenRequested(Some(id))],
        "with no menu open it is the picker's"
    );
    for (how, launch) in launches() {
        let (mut world, id) = world_with_picker(3);
        let mut shown = Shown::new(&mut world, id).with_tokens();
        let popup = open_menu(&mut shown, 0);
        launch(&mut shown);
        assert_eq!(
            shown.seen,
            [
                Seen::Opened(popup),
                Seen::TokenRequested(Some(popup)),
                Seen::Destroyed(popup)
            ],
            "{how}"
        );
        assert!(is_open(&shown), "{how}: the picker stays for its launch");
    }
}

#[test]
fn the_popup_of_a_menu_launch_stays_exactly_as_long_as_its_token_is_awaited() {
    if !in_scratch_home("the_popup_of_a_menu_launch_stays_exactly_as_long_as_its_token_is_awaited")
    {
        return;
    }
    let (mut world, id) = world_with_picker(3);
    let mut shown = Shown::new(&mut world, id);
    let popup = open_menu(&mut shown, 0);
    shown.press_in_popup(press(Named::Enter));
    assert_eq!(shown.picker().menu, None, "the menu is closed");
    assert_eq!(
        shown.seen,
        [Seen::Opened(popup), Seen::TokenRequested(Some(popup))],
        "and its popup waits for the token"
    );
    let attempt = pending(&shown).attempt;
    shown.update(Message::TokenTimeout { id, attempt });
    assert_eq!(
        shown.seen,
        [
            Seen::Opened(popup),
            Seen::TokenRequested(Some(popup)),
            Seen::Destroyed(popup)
        ],
        "it goes when the app stops waiting"
    );

    let (mut world, id) = world_with_picker(3);
    world.app.pickers.get_mut(&id).expect("its state").pending = Some(Pending {
        attempt: 7,
        index: 1,
        action: None,
        keep_open: false,
        launching: true,
        popup: None,
    });
    let mut shown = Shown::new(&mut world, id);
    let popup = open_menu(&mut shown, 0);
    shown.press_in_popup(press(Named::Enter));
    assert_eq!(
        shown.seen,
        [Seen::Opened(popup), Seen::Destroyed(popup)],
        "a launch the picker does not admit asks for no token, and nothing keeps its popup"
    );
}

#[test]
fn esc_in_the_popup_a_launch_waits_with_destroys_it_at_once_and_the_launch_goes_on() {
    if !in_scratch_home(
        "esc_in_the_popup_a_launch_waits_with_destroys_it_at_once_and_the_launch_goes_on",
    ) {
        return;
    }
    for (how, launch) in launches() {
        let (mut world, id) = world_with_picker(3);
        let mut shown = Shown::new(&mut world, id);
        let popup = open_menu(&mut shown, 0);
        launch(&mut shown);
        let attempt = pending(&shown).attempt;
        shown.press_in_popup(press(Named::Escape));
        assert_eq!(
            shown.seen,
            [
                Seen::Opened(popup),
                Seen::TokenRequested(Some(popup)),
                Seen::Destroyed(popup)
            ],
            "{how}: Esc takes the popup and nothing else"
        );
        assert!(is_open(&shown), "{how}: the picker stays");
        assert!(
            shown.world.app.pickers[&id].awaiting_token(attempt),
            "{how}: the launch still waits for its token"
        );
        shown.update(Message::TokenTimeout { id, attempt });
        assert_eq!(shown.seen.len(), 3, "{how}: nothing is destroyed twice");
    }
}

#[test]
fn a_menu_opened_while_a_launch_waits_for_its_token_never_leaves_two_popups_alive() {
    if !in_scratch_home(
        "a_menu_opened_while_a_launch_waits_for_its_token_never_leaves_two_popups_alive",
    ) {
        return;
    }
    for (how, launch) in launches() {
        let (mut world, id) = world_with_picker(3);
        let mut shown = Shown::new(&mut world, id);
        open_menu(&mut shown, 0);
        launch(&mut shown);
        let attempt = pending(&shown).attempt;
        let second = open_menu(&mut shown, 1);
        assert_eq!(alive(&shown), [second], "{how}: the new menu's alone");
        shown.update(Message::TokenTimeout { id, attempt });
        assert_eq!(
            alive(&shown),
            [second],
            "{how}: the token's end leaves the new menu's popup be"
        );
    }
}

#[test]
fn keys_and_presses_addressed_to_the_popup_a_launch_waits_with_act_on_its_picker() {
    if !in_scratch_home(
        "keys_and_presses_addressed_to_the_popup_a_launch_waits_with_act_on_its_picker",
    ) {
        return;
    }
    let (mut world, id) = world_with_picker(3);
    let mut shown = Shown::new(&mut world, id);
    open_menu(&mut shown, 0);
    shown.press_in_popup(press(Named::Enter));
    assert_eq!(shown.picker().menu, None, "the launch closed the menu");
    shown.press_in_popup(press(Named::ArrowRight));
    assert_eq!(
        shown.picker().focus,
        1,
        "the arrow moved the picker's focus"
    );
    assert!(shown.picker().focus_visible);
    shown.receive_in_popup(left_press());
    assert!(!shown.picker().focus_visible, "the press hid it");
}

#[test]
fn a_picker_that_closes_while_its_menu_launch_waits_destroys_the_popup_first() {
    if !in_scratch_home("a_picker_that_closes_while_its_menu_launch_waits_destroys_the_popup_first")
    {
        return;
    }
    let (mut world, id) = world_with_picker(3);
    let mut shown = Shown::new(&mut world, id);
    let popup = open_menu(&mut shown, 0);
    shown.press_in_popup(press(Named::Enter));
    shown.update(Message::Shutdown);
    assert_eq!(
        shown.seen,
        [
            Seen::Opened(popup),
            Seen::TokenRequested(Some(popup)),
            Seen::Destroyed(popup),
            Seen::LayerDestroyed(id)
        ]
    );
}

#[test]
fn closing_the_picker_destroys_the_popup_before_the_picker() {
    if !in_scratch_home("closing_the_picker_destroys_the_popup_before_the_picker") {
        return;
    }
    let closers: [(&str, Close); 2] = [
        ("skipping to the next link", |id| {
            Message::Tile(id, Input::SkipToNext)
        }),
        ("the daemon shutting down", |_| Message::Shutdown),
    ];
    for (how, close) in closers {
        let (mut world, id) = world_with_picker(3);
        let mut shown = Shown::new(&mut world, id);
        let popup = open_menu(&mut shown, 0);
        shown.update(close(id));
        assert_eq!(
            shown.seen,
            [
                Seen::Opened(popup),
                Seen::Destroyed(popup),
                Seen::LayerDestroyed(id)
            ],
            "{how}"
        );
    }
}

#[test]
fn a_menu_closed_before_it_was_placed_gets_no_popup_and_one_no_tile_answered_for_gets_none_either()
{
    if !in_scratch_home(
        "a_menu_closed_before_it_was_placed_gets_no_popup_and_one_no_tile_answered_for_gets_none_either",
    ) {
        return;
    }
    let (mut world, id) = world_with_picker(3);
    let mut shown = Shown::new(&mut world, id);
    let tile = Some(Rectangle::new(Point::ORIGIN, Size::new(10.0, 10.0)));
    let anchored = |tile_index, bounds| Message::MenuAnchored {
        id,
        tile: tile_index,
        bounds,
    };
    shown.update(anchored(0, tile));
    assert!(shown.requests.is_empty(), "no menu is open");

    open_menu(&mut shown, 1);
    for (what, late) in [
        ("another tile's", anchored(0, tile)),
        ("a repeat of its own", anchored(1, tile)),
    ] {
        shown.update(late);
        assert_eq!(
            shown.requests.len(),
            1,
            "{what} answer opens no second popup"
        );
    }

    shown.input(Input::CloseMenu);
    let model_only = |shown: &mut Shown<'_>| {
        shown
            .world
            .app
            .picker_mut(id)
            .expect("the picker")
            .handle(Input::OpenMenu(Some(2)));
    };
    model_only(&mut shown);
    shown.update(anchored(2, None));
    assert_eq!(
        shown.picker().menu,
        None,
        "a menu no tile can anchor is not left open"
    );
    assert_eq!(shown.requests.len(), 1);
}

/// Whether `anchor` is `tile`, in whole pixels.
fn is_at(anchor: Rectangle<i32>, tile: Rectangle) -> bool {
    [
        (anchor.x, tile.x),
        (anchor.y, tile.y),
        (anchor.width, tile.width),
        (anchor.height, tile.height),
    ]
    .iter()
    .all(|(whole, laid_out)| (f64::from(*whole) - f64::from(*laid_out)).abs() <= ROUNDING)
}

#[test]
fn the_popup_is_anchored_to_the_scrolled_tile_and_parented_to_the_picker() {
    if !in_scratch_home("the_popup_is_anchored_to_the_scrolled_tile_and_parented_to_the_picker") {
        return;
    }
    let (mut world, id) = world_with_picker(TALL);
    let mut shown = Shown::new(&mut world, id);
    assert!(shown.wheel_to_bottom() > 0.0, "the user scrolled the grid");
    let unscrolled = shown.tile_on_surface(TALL - 1);
    open_menu(&mut shown, TALL - 1);

    let [request] = &shown.requests[..] else {
        panic!("one popup is asked for: {:?}", shown.requests);
    };
    let settings = request.settings.clone();
    assert_eq!(settings.parent, id, "a child of the picker's layer");
    assert!(settings.grab, "the popup takes the grab");
    let tile = shown.tile_on_surface(TALL - 1);
    let anchor = settings.positioner.anchor_rect;
    assert!(
        is_at(anchor, tile),
        "{anchor:?} is not the tile at {tile:?}"
    );
    assert!(
        near(unscrolled.y, tile.y),
        "the grid did not move for a pointer"
    );
    let positioner = settings.positioner;
    assert_eq!(
        (positioner.anchor, positioner.gravity),
        (Anchor::TopRight, Gravity::BottomRight),
        "beside the tile, from its top"
    );
    assert!(
        (f64::from(positioner.offset.0) - f64::from(MENU_GAP)).abs() <= ROUNDING
            && positioner.offset.1 == 0,
        "a gap away: {:?}",
        positioner.offset
    );
}

#[test]
fn a_menu_the_keyboard_opens_waits_for_the_grid_to_show_its_tile() {
    if !in_scratch_home("a_menu_the_keyboard_opens_waits_for_the_grid_to_show_its_tile") {
        return;
    }
    let (mut world, id) = world_with_picker(TALL);
    let mut shown = Shown::new(&mut world, id);
    for _ in 0..4 {
        shown.receive(press(Named::ArrowDown));
    }
    let focused = shown.picker().focus;
    shown.wheel_to_top();
    assert!(
        near(shown.grid_offset(), 0.0),
        "the user scrolled away from it"
    );

    shown.receive(press(Named::ContextMenu));
    assert!(
        shown.grid_offset() > 0.0,
        "the grid scrolled to the tile first"
    );
    let [request] = &shown.requests[..] else {
        panic!("one popup is asked for: {:?}", shown.requests);
    };
    let anchor = request.settings.positioner.anchor_rect;
    let tile = shown.tile_on_surface(focused);
    assert!(
        is_at(anchor, tile),
        "{anchor:?} is not the tile as shown, {tile:?}"
    );
}

#[test]
fn a_banner_and_open_details_move_the_anchor_with_the_tiles() {
    if !in_scratch_home("a_banner_and_open_details_move_the_anchor_with_the_tiles") {
        return;
    }
    let (mut world, id) = world_with_picker(5);
    let mut shown = Shown::new(&mut world, id);
    open_menu(&mut shown, 4);
    let plain = shown.requests[0].settings.positioner.anchor_rect;
    shown.input(Input::CloseMenu);

    let picker = shown.world.app.picker_mut(id).expect("the picker");
    picker.fail("exited\n".repeat(30), None);
    picker.handle(Input::ToggleDetails);
    open_menu(&mut shown, 4);
    let failed = shown.requests[1].settings.positioner.anchor_rect;
    let tile = shown.tile_on_surface(4);
    assert!(
        failed.y > plain.y,
        "{failed:?} is not under the banner, {plain:?}"
    );
    assert!(
        is_at(failed, tile),
        "{failed:?} is not the tile at {tile:?}"
    );
}

/// The size of the popup the app asks for when tile 0 offers `actions`, once every row of its menu lies whole
/// inside it.
fn popup_size_for(actions: Vec<TileAction>) -> Size {
    let mut tile = plain_tile();
    tile.actions = actions;
    let (mut world, id) = world_with_picker(1);
    world.app.picker_mut(id).expect("the picker").tiles = vec![tile];
    let mut shown = Shown::new(&mut world, id);
    open_menu(&mut shown, 0);
    let popup = Rectangle::with_size(shown.popup_size());
    let rows = shown.popup_probe().stops;
    assert_eq!(rows.len(), shown.picker().menu_items().len());
    for row in rows {
        assert!(
            lies_inside(row, popup),
            "{row:?} is not inside the popup {popup:?}"
        );
    }
    shown.popup_size()
}

#[test]
fn the_popup_sizes_itself_to_its_menu_with_no_blur() {
    if !in_scratch_home("the_popup_sizes_itself_to_its_menu_with_no_blur") {
        return;
    }
    let (mut world, id) = world_with_picker(1);
    let mut shown = Shown::new(&mut world, id);
    open_menu(&mut shown, 0);

    let request = &shown.requests[0];
    assert_eq!(
        request.live.blur,
        Some(false),
        "an opaque popup asks for none"
    );
    assert_eq!(
        request.settings.positioner.size, None,
        "no constant: its widgets say how big it is"
    );
}

#[test]
fn the_popup_is_as_big_as_the_rows_of_its_menu_need() {
    if !in_scratch_home("the_popup_is_as_big_as_the_rows_of_its_menu_need") {
        return;
    }
    let short = popup_size_for(Vec::new());
    let taller = popup_size_for(vec![TileAction::Private; 5]);
    assert!(taller.height > short.height, "{taller:?} against {short:?}");
    let label = "Open a new window in a temporary profile with every extension turned off";
    let wider = popup_size_for(vec![TileAction::Desktop {
        id: "long".into(),
        label: label.into(),
    }]);
    assert!(wider.width > short.width, "{wider:?} against {short:?}");
}

#[test]
fn a_menu_taller_than_the_popup_scrolls_in_it_to_the_row_the_keyboard_reaches() {
    if !in_scratch_home(
        "a_menu_taller_than_the_popup_scrolls_in_it_to_the_row_the_keyboard_reaches",
    ) {
        return;
    }
    let mut tile = plain_tile();
    tile.actions = vec![TileAction::Private; 40];
    let (mut world, id) = world_with_picker(1);
    world.app.picker_mut(id).expect("the picker").tiles = vec![tile];
    let mut shown = Shown::new(&mut world, id);
    open_menu(&mut shown, 0);
    assert!(near(shown.menu_offset(), 0.0), "it opens at its first row");
    for _ in 0..30 {
        shown.press_in_popup(press(Named::ArrowDown));
    }
    assert_eq!(shown.picker().menu_focus, 30);
    assert!(shown.menu_offset() > 0.0, "the list scrolled to the row");
}

/// Labels of one, two and three lines: a desktop entry can name an action with line breaks in it.
const LABELS: [&str; 4] = [
    "Short",
    "A label a good deal longer than the first",
    "Two\nlines",
    "Three\nlines\nin all",
];

#[test]
fn the_keyboard_keeps_the_chosen_row_wholly_in_view_whatever_the_heights_of_the_labels() {
    if !in_scratch_home(
        "the_keyboard_keeps_the_chosen_row_wholly_in_view_whatever_the_heights_of_the_labels",
    ) {
        return;
    }
    let mut tile = plain_tile();
    tile.actions = (0..40)
        .map(|number| TileAction::Desktop {
            id: format!("action-{number}"),
            label: format!("{number} {}", LABELS[number % LABELS.len()]),
        })
        .collect();
    let (mut world, id) = world_with_picker(1);
    world.app.picker_mut(id).expect("the picker").tiles = vec![tile];
    let mut shown = Shown::new(&mut world, id);
    open_menu(&mut shown, 0);
    let steps = shown.picker().menu_items().len() - 1;
    let down = std::iter::repeat_n(Named::ArrowDown, steps);
    let up = std::iter::repeat_n(Named::ArrowUp, steps);
    for (step, key) in down.chain(up).enumerate() {
        shown.press_in_popup(press(key));
        let probe = shown.popup_probe();
        let list = probe.scrollable_named(&MENU_SCROLL).expect("the list");
        assert!(list.content.height > list.bounds.height, "a menu to scroll");
        let row = probe.stops[shown.picker().menu_focus] - list.offset;
        assert!(
            lies_inside(row, list.bounds),
            "step {step} to row {}: {row:?} is not inside the list {:?}",
            shown.picker().menu_focus,
            list.bounds
        );
    }
}

#[test]
fn the_popup_draws_the_same_whatever_theme_the_app_draws_in() {
    if !in_scratch_home("the_popup_draws_the_same_whatever_theme_the_app_draws_in") {
        return;
    }
    let (mut world, id) = world_with_picker(3);
    let mut shown = Shown::new(&mut world, id);
    open_menu(&mut shown, 0);
    let frosted = shown.popup_drawn(&translucent(true, FROSTED_ALPHA));
    let opaque = shown.popup_drawn(&cosmic::Theme::dark());
    assert!(
        frosted == opaque,
        "the popup reads a theme of its own, not the app's"
    );
}

#[test]
fn reclaiming_the_keyboard_clears_the_native_focus_of_the_popup_too() {
    if !in_scratch_home("reclaiming_the_keyboard_clears_the_native_focus_of_the_popup_too") {
        return;
    }
    let (mut world, id) = world_with_picker(3);
    let mut shown = Shown::new(&mut world, id).with_settings();
    open_menu(&mut shown, 0);
    shown.tab_in_settings();
    shown.tab_in_popup();
    assert_eq!(
        shown.popup_focused().len(),
        1,
        "a row of the menu has a focus"
    );
    assert_eq!(shown.settings_stops().1.len(), 1, "so do the settings");

    shown.press_in_popup(press(Named::ArrowDown));
    assert!(
        shown.popup_focused().is_empty(),
        "the arrow took the menu's rows out of the focus"
    );
    assert_eq!(
        shown.settings_stops().1.len(),
        1,
        "and left the settings window's focus alone"
    );
    assert_eq!(
        shown.popup_answer(press(Named::Enter), Point::ORIGIN).1,
        Status::Ignored,
        "no row is left to take Enter"
    );
}
