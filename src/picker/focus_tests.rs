//! When the picker shows its focus: only once the keyboard has moved it, and not after the pointer took over.

use super::tests::{URI, menu_picker, picker, tiles};
use super::*;

fn launches(index: usize) -> Outcome {
    Outcome::Launch {
        index,
        action: None,
        keep_open: false,
    }
}

#[test]
fn a_fresh_picker_shows_no_focus_yet_enter_still_launches_its_first_tile() {
    let mut p = picker(3);
    assert!(!p.focus_visible);
    assert_eq!(p.handle(Input::Activate { ctrl: false }), launches(0));
}

#[test]
fn an_arrow_shows_the_focus_even_where_it_cannot_move() {
    let mut p = picker(3);
    p.handle(Input::Move(-1, 0));
    assert_eq!((p.focus, p.focus_visible), (0, true), "clamped at the edge");
    let mut p = picker(3);
    p.handle(Input::Move(1, 0));
    assert_eq!((p.focus, p.focus_visible), (1, true));
}

#[test]
fn a_digit_shows_the_tile_it_picks_and_a_digit_without_a_tile_shows_nothing() {
    let mut p = picker(3);
    p.handle(Input::DigitPress(9));
    assert!(!p.focus_visible, "no ninth tile");
    p.handle(Input::DigitPress(2));
    assert_eq!((p.focus, p.focus_visible), (1, true));
    assert_eq!(p.handle(Input::DigitRelease(2)), launches(1));
}

#[test]
fn the_menu_key_shows_the_first_row_but_a_pointer_that_opens_the_menu_does_not() {
    let mut p = picker(3);
    p.handle(Input::OpenMenu(None));
    assert_eq!((p.menu, p.menu_focus, p.focus_visible), (Some(0), 0, true));

    let mut p = picker(3);
    p.handle(Input::Move(1, 0));
    p.handle(Input::OpenMenu(Some(1)));
    assert_eq!((p.menu, p.menu_focus, p.focus_visible), (Some(1), 0, false));
}

#[test]
fn a_long_press_opens_its_menu_with_the_focus_hidden_even_after_a_key_in_between() {
    let mut p = picker(3);
    let Outcome::ArmLongPress(seq) = p.handle(Input::PressStart(1)) else {
        panic!("a press arms the long-press")
    };
    p.handle(Input::Move(0, 0));
    assert!(p.focus_visible, "the key showed the focus again");
    p.handle(Input::LongPress(seq));
    assert_eq!((p.menu, p.menu_focus, p.focus_visible), (Some(1), 0, false));
}

#[test]
fn a_press_hides_the_focus_without_forgetting_where_it_was() {
    let mut p = picker(3);
    p.handle(Input::Move(1, 0));
    p.handle(Input::PointerDown);
    assert_eq!((p.focus, p.focus_visible), (1, false));
    assert_eq!(
        p.handle(Input::Activate { ctrl: false }),
        launches(1),
        "Enter keeps its target"
    );
    p.handle(Input::Move(1, 0));
    assert_eq!(
        (p.focus, p.focus_visible),
        (2, true),
        "the arrows go on from it"
    );
}

#[test]
fn every_gesture_that_presses_a_tile_or_the_card_hides_the_focus() {
    let gestures = [
        Input::PointerDown,
        Input::PressStart(2),
        Input::Click {
            index: 2,
            keep_open: true,
        },
        Input::OpenMenu(Some(2)),
        Input::CloseMenu,
    ];
    for gesture in gestures {
        let mut p = picker(3);
        p.handle(Input::Move(1, 0));
        assert!(p.focus_visible, "the arrow showed the focus");
        p.handle(gesture.clone());
        assert!(!p.focus_visible, "{gesture:?}");
    }
}

#[test]
fn hovering_leaves_the_focus_as_it_was() {
    for shown in [false, true] {
        let mut p = picker(3);
        if shown {
            p.handle(Input::Move(1, 0));
        }
        for hover in [Input::HoverStart(2), Input::HoverEnd(2), Input::HoverClear] {
            p.handle(hover);
            assert_eq!(p.focus_visible, shown);
        }
    }
}

#[test]
fn closing_the_menu_leaves_the_focus_shown_or_hidden_as_it_was() {
    let mut p = picker(3);
    p.handle(Input::Move(1, 0));
    p.handle(Input::OpenMenu(None));
    p.handle(Input::Escape);
    assert_eq!(
        (p.menu, p.focus_visible),
        (None, true),
        "keyboard, then Esc"
    );

    p.handle(Input::PointerDown);
    p.handle(Input::OpenMenu(Some(1)));
    p.handle(Input::Escape);
    assert_eq!(
        (p.menu, p.focus_visible),
        (None, false),
        "pointer, then Esc"
    );
}

#[test]
fn arrows_reveal_the_row_of_a_menu_the_pointer_opened() {
    let mut p = menu_picker();
    assert!(!p.focus_visible);
    p.handle(Input::Move(0, 1));
    assert_eq!((p.menu_focus, p.focus_visible), (1, true));
}

#[test]
fn tab_outside_the_menu_hands_the_focus_to_the_native_controls() {
    let mut p = picker(3);
    p.handle(Input::Move(1, 0));
    assert!(p.focus_visible, "the arrow showed the focus");
    p.handle(Input::Tab { shift: false });
    assert_eq!(
        (p.focus, p.focus_visible),
        (1, false),
        "not forgotten, not shown"
    );
    assert_eq!(p.handle(Input::Activate { ctrl: false }), launches(1));
    p.handle(Input::Move(0, 0));
    p.handle(Input::Tab { shift: true });
    assert!(!p.focus_visible);
}

#[test]
fn tab_walks_the_rows_of_an_open_menu_round_and_round() {
    let mut p = Picker::new(URI.into(), tiles(1, false));
    p.handle(Input::OpenMenu(Some(0)));
    let rows = p.menu_items().len();
    assert_eq!(rows, 3, "Open, Open and keep, Copy");
    p.handle(Input::Tab { shift: false });
    assert_eq!((p.menu_focus, p.focus_visible), (1, true));
    p.handle(Input::Tab { shift: false });
    p.handle(Input::Tab { shift: false });
    assert_eq!(p.menu_focus, 0, "past the last row it comes round");
    p.handle(Input::Tab { shift: true });
    assert_eq!(p.menu_focus, rows - 1, "and back from the first row");
    p.handle(Input::Tab { shift: true });
    assert_eq!(p.menu_focus, 1);
    assert_eq!(p.menu, Some(0), "Tab leaves the menu open");
}

#[test]
fn only_a_key_that_moves_the_focus_navigates() {
    let navigating = [
        Input::Move(0, 0),
        Input::DigitPress(1),
        Input::OpenMenu(None),
        Input::Tab { shift: false },
    ];
    let others = [
        Input::DigitRelease(1),
        Input::Activate { ctrl: false },
        Input::OpenMenu(Some(0)),
        Input::PointerDown,
        Input::HoverStart(0),
        Input::Escape,
    ];
    assert!(navigating.iter().all(Input::is_navigation));
    assert!(others.iter().all(|input| !input.is_navigation()));
}

#[test]
fn tab_maps_to_the_input_and_shift_turns_it_around() {
    assert_eq!(
        map_key(Key::Tab, false, true),
        Some(Input::Tab { shift: false })
    );
    assert_eq!(map_key(Key::Tab, false, false), None);
    assert_eq!(
        map_key(Key::Tab, true, true),
        None,
        "Ctrl+Tab is no traversal"
    );
    assert_eq!(
        Input::Tab { shift: false }.with_shift(true),
        Input::Tab { shift: true }
    );
    assert_eq!(Input::Escape.with_shift(true), Input::Escape);
}
