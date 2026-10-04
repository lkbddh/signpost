use std::path::PathBuf;

use super::*;
use crate::profiles::Profile;

pub(super) const URI: &str = "https://x/";

pub(super) fn tile(app: &str, profile: Option<&str>) -> Tile {
    Tile {
        app_id: app.into(),
        app_name: app.into(),
        label: profile.unwrap_or(app).into(),
        icon: None,
        profile: profile.map(|key| Profile {
            key: key.into(),
            label: key.into(),
            dir: PathBuf::new(),
            color: None,
        }),
        flatpak: false,
        actions: vec![TileAction::Private],
    }
}

/// `n` profile tiles of one app.
pub(super) fn tiles(n: usize, with_actions: bool) -> Vec<Tile> {
    (0..n)
        .map(|i| Tile {
            actions: if with_actions {
                vec![TileAction::Private]
            } else {
                vec![]
            },
            ..tile("app", Some(&format!("p{i}")))
        })
        .collect()
}

/// Consecutive groups of profile tiles, one group per app.
pub(super) fn apps(groups: &[(&str, usize)]) -> Vec<Tile> {
    groups
        .iter()
        .flat_map(|&(app, n)| (0..n).map(move |i| tile(app, Some(&format!("p{i}")))))
        .collect()
}

pub(super) fn new_window() -> TileAction {
    TileAction::Desktop {
        id: "new-window".into(),
        label: "New Window".into(),
    }
}

/// Two tiles; the menu of tile 1 (`new_window` then `Private`, as `tiles_for` orders them) is open.
pub(super) fn menu_picker() -> Picker {
    let mut ts = tiles(2, true);
    ts[1].actions = vec![new_window(), TileAction::Private];
    let mut p = Picker::new(URI.into(), ts);
    p.handle(Input::OpenMenu(Some(1)));
    p
}

pub(super) fn picker(n: usize) -> Picker {
    Picker::new(URI.into(), tiles(n, true))
}

#[test]
fn arrows_move_within_the_grid_and_clamp() {
    let mut p = picker(6);
    p.handle(Input::Move(1, 0));
    assert_eq!(p.focus, 1);
    p.handle(Input::Move(0, 1));
    assert_eq!(p.focus, 5);
    p.handle(Input::Move(0, 1));
    assert_eq!(p.focus, 5, "no row below");
    p.handle(Input::Move(-1, 0));
    p.handle(Input::Move(-1, 0));
    p.handle(Input::Move(-1, 0));
    p.handle(Input::Move(-1, 0));
    assert_eq!(p.focus, 4, "row 1 clamps at column 0");
    p.handle(Input::Move(-5, -5));
    assert_eq!(p.focus, 0);
}

#[test]
fn enter_launches_focus_and_ctrl_keeps_open() {
    let mut p = picker(3);
    p.handle(Input::Move(2, 0));
    assert_eq!(
        p.handle(Input::Activate { ctrl: false }),
        Outcome::Launch {
            index: 2,
            action: None,
            keep_open: false
        }
    );
    assert_eq!(
        p.handle(Input::Activate { ctrl: true }),
        Outcome::Launch {
            index: 2,
            action: None,
            keep_open: true
        }
    );
}

#[test]
fn digits_focus_on_press_and_launch_on_release_of_the_same_digit() {
    let mut p = picker(3);
    assert_eq!(p.handle(Input::DigitPress(3)), Outcome::None);
    assert_eq!(p.focus, 2);
    assert_eq!(p.handle(Input::DigitRelease(2)), Outcome::None);
    assert_eq!(
        p.handle(Input::DigitRelease(3)),
        Outcome::Launch {
            index: 2,
            action: None,
            keep_open: false
        }
    );
    assert_eq!(p.handle(Input::DigitPress(9)), Outcome::None);
    assert_eq!(p.focus, 2, "digit beyond tiles is ignored");
}

#[test]
fn digit_release_needs_the_armed_digit_and_focus() {
    let mut p = picker(3);
    assert_eq!(
        p.handle(Input::DigitRelease(1)),
        Outcome::None,
        "unarmed release"
    );
    p.handle(Input::DigitPress(3));
    p.handle(Input::Move(-1, 0));
    assert_eq!(
        p.handle(Input::DigitRelease(2)),
        Outcome::None,
        "mismatched digit"
    );
    p.handle(Input::DigitPress(3));
    p.handle(Input::Move(-1, 0));
    assert_eq!(
        p.handle(Input::DigitRelease(3)),
        Outcome::None,
        "focus moved away after the press"
    );
    p.handle(Input::DigitPress(2));
    assert_eq!(
        p.handle(Input::DigitRelease(2)),
        Outcome::Launch {
            index: 1,
            action: None,
            keep_open: false
        }
    );
}

#[test]
fn press_release_is_a_click_and_long_press_opens_menu_without_launching() {
    let mut p = picker(2);
    let Outcome::ArmLongPress(_) = p.handle(Input::PressStart(1)) else {
        panic!("must arm")
    };
    assert_eq!(
        p.handle(Input::PressEnd {
            index: 1,
            keep_open: true
        }),
        Outcome::Launch {
            index: 1,
            action: None,
            keep_open: true
        }
    );
    let Outcome::ArmLongPress(seq) = p.handle(Input::PressStart(1)) else {
        panic!("must arm")
    };
    assert_eq!(p.handle(Input::LongPress(seq)), Outcome::None);
    assert_eq!(p.menu, Some(1));
    assert_eq!(
        p.handle(Input::PressEnd {
            index: 1,
            keep_open: false
        }),
        Outcome::None,
        "release after a long-press does not launch"
    );
}

#[test]
fn short_cancelled_or_stale_presses_do_not_open_the_menu() {
    let mut p = picker(2);
    let Outcome::ArmLongPress(seq) = p.handle(Input::PressStart(0)) else {
        panic!()
    };
    assert_eq!(
        p.handle(Input::PressEnd {
            index: 0,
            keep_open: false
        }),
        Outcome::Launch {
            index: 0,
            action: None,
            keep_open: false
        }
    );
    p.handle(Input::LongPress(seq));
    assert_eq!(p.menu, None, "timer fired after release");
    let Outcome::ArmLongPress(seq) = p.handle(Input::PressStart(0)) else {
        panic!()
    };
    p.handle(Input::PressCancel);
    p.handle(Input::LongPress(seq));
    assert_eq!(p.menu, None, "pointer left the tile");
    assert_eq!(
        p.handle(Input::PressEnd {
            index: 0,
            keep_open: false
        }),
        Outcome::None,
        "no launch after a cancelled press"
    );
    let Outcome::ArmLongPress(old) = p.handle(Input::PressStart(0)) else {
        panic!()
    };
    p.handle(Input::PressCancel);
    let Outcome::ArmLongPress(_new) = p.handle(Input::PressStart(0)) else {
        panic!()
    };
    p.handle(Input::LongPress(old));
    assert_eq!(p.menu, None, "stale timer from an earlier press");
}

/// A release counts only over the tile it is held on, so a first finger lifting from its tile cannot launch the
/// tile a second finger holds.
#[test]
fn a_finger_lifted_from_another_tile_launches_nothing() {
    let mut p = picker(2);
    p.handle(Input::PressStart(0));
    p.handle(Input::PressStart(1));
    let lift = |index| Input::PressEnd {
        index,
        keep_open: false,
    };
    assert_eq!(p.handle(lift(0)), Outcome::None, "the first finger lifts");
    assert_eq!(
        p.handle(lift(1)),
        Outcome::Launch {
            index: 1,
            action: None,
            keep_open: false
        },
        "the second finger lifts"
    );
}

/// However the menu opens over a held tile, the release that follows it launches nothing.
#[test]
fn a_menu_opened_while_a_tile_is_held_swallows_the_release() {
    for open in [Input::OpenMenu(None), Input::OpenMenu(Some(1))] {
        let mut p = picker(2);
        p.handle(Input::PressStart(1));
        p.handle(open.clone());
        p.handle(Input::Escape);
        assert_eq!(p.menu, None);
        assert_eq!(
            p.handle(Input::PressEnd {
                index: 1,
                keep_open: false
            }),
            Outcome::None,
            "{open:?}"
        );
    }
}

#[test]
fn hover_follows_the_pointer_and_never_launches() {
    let mut p = picker(3);
    assert_eq!(p.hover, None);
    assert_eq!(p.handle(Input::HoverStart(2)), Outcome::None);
    assert_eq!(p.hover, Some(2));
    assert_eq!(p.focus, 0, "the pointer does not move the keyboard focus");
    assert_eq!(p.handle(Input::HoverEnd(2)), Outcome::None);
    assert_eq!(p.hover, None);
}

#[test]
fn the_pointer_reports_an_exit_twice_and_the_second_changes_nothing() {
    let mut p = picker(3);
    p.handle(Input::HoverStart(1));
    p.handle(Input::HoverEnd(1));
    p.handle(Input::HoverStart(2));
    p.handle(Input::HoverEnd(1));
    assert_eq!(p.hover, Some(2), "the late exit of tile 1 is not tile 2's");
}

#[test]
fn a_press_survives_the_exit_of_another_tile() {
    let mut p = picker(3);
    p.handle(Input::HoverStart(0));
    p.handle(Input::PressStart(0));
    p.handle(Input::HoverEnd(1));
    assert_eq!(
        p.handle(Input::PressEnd {
            index: 0,
            keep_open: false
        }),
        Outcome::Launch {
            index: 0,
            action: None,
            keep_open: false
        }
    );
}

#[test]
fn leaving_the_pressed_tile_cancels_its_press() {
    let mut p = picker(3);
    let Outcome::ArmLongPress(seq) = p.handle(Input::PressStart(0)) else {
        panic!("must arm")
    };
    p.handle(Input::HoverEnd(0));
    p.handle(Input::LongPress(seq));
    assert_eq!(p.menu, None, "the pointer left before the timer");
    assert_eq!(
        p.handle(Input::PressEnd {
            index: 0,
            keep_open: false
        }),
        Outcome::None
    );
}

#[test]
fn leaving_the_grid_clears_the_hover_and_a_press_under_way() {
    let mut p = picker(3);
    p.handle(Input::HoverStart(1));
    p.handle(Input::PressStart(1));
    assert_eq!(p.handle(Input::HoverClear), Outcome::None);
    assert_eq!(p.hover, None);
    assert_eq!(
        p.handle(Input::PressEnd {
            index: 1,
            keep_open: false
        }),
        Outcome::None,
        "no click from a press the pointer walked away from"
    );
    assert_eq!(p.focus, 0);
}

#[test]
fn enter_launches_the_keyboard_focus_whatever_the_pointer_is_over() {
    let mut p = picker(3);
    p.handle(Input::Move(1, 0));
    p.handle(Input::HoverStart(2));
    assert_eq!(
        p.handle(Input::Activate { ctrl: false }),
        Outcome::Launch {
            index: 1,
            action: None,
            keep_open: false
        }
    );
}

#[test]
fn long_press_opens_a_menu_on_a_tile_without_actions_and_does_not_launch() {
    let mut p = Picker::new(URI.into(), tiles(1, false));
    let Outcome::ArmLongPress(seq) = p.handle(Input::PressStart(0)) else {
        panic!("must arm")
    };
    assert_eq!(p.handle(Input::LongPress(seq)), Outcome::None);
    assert_eq!(p.menu, Some(0), "every tile has a menu");
    assert_eq!(
        p.handle(Input::PressEnd {
            index: 0,
            keep_open: false
        }),
        Outcome::None,
        "release after a long-press never launches"
    );
    p.handle(Input::CloseMenu);
    let Outcome::ArmLongPress(_) = p.handle(Input::PressStart(0)) else {
        panic!("must arm")
    };
    assert_eq!(
        p.handle(Input::PressEnd {
            index: 0,
            keep_open: false
        }),
        Outcome::Launch {
            index: 0,
            action: None,
            keep_open: false
        },
        "a short press afterwards is a normal click again"
    );
}

#[test]
fn focus_loss_drops_an_armed_digit_even_when_the_picker_stays() {
    let mut p = picker(2);
    p.keep_open = true;
    p.handle(Input::DigitPress(1));
    assert_eq!(p.handle(Input::FocusLost), Outcome::None);
    assert_eq!(
        p.handle(Input::DigitRelease(1)),
        Outcome::None,
        "digit armed before the focus loss"
    );
}

#[test]
fn focus_loss_drops_a_pending_press_even_when_the_picker_stays() {
    let mut p = picker(2);
    p.keep_open = true;
    let Outcome::ArmLongPress(seq) = p.handle(Input::PressStart(0)) else {
        panic!("must arm")
    };
    assert_eq!(p.handle(Input::FocusLost), Outcome::None);
    assert_eq!(p.handle(Input::LongPress(seq)), Outcome::None);
    assert_eq!(p.menu, None, "timer of a press from before the focus loss");
    assert_eq!(
        p.handle(Input::PressEnd {
            index: 0,
            keep_open: false
        }),
        Outcome::None,
        "release of a press from before the focus loss"
    );
}

#[test]
fn links_are_presented_one_at_a_time_in_order() {
    let link = |u: &str| QueuedLink {
        uri: u.into(),
        error: None,
        failed: None,
    };
    let mut q = LinkQueue::default();
    assert_eq!(q.push(link("a")), Some(link("a")));
    assert_eq!(q.push(link("b")), None);
    assert_eq!(q.push(link("c")), None);
    assert_eq!(q.waiting(), 2);
    q.closing();
    assert_eq!(q.gone(), Some(link("b")));
    q.closing();
    assert_eq!(q.gone(), Some(link("c")));
    q.closing();
    assert_eq!(q.gone(), None);
    assert_eq!(
        q.push(link("d")),
        Some(link("d")),
        "idle again after the queue drains"
    );
}

#[test]
fn a_closing_picker_holds_back_the_next_link_even_when_none_waits() {
    let mut q = LinkQueue::default();
    assert_eq!(q.push(queued("a")), Some(queued("a")));
    q.closing();
    assert_eq!(q.push(queued("b")), None, "a is still on screen");
    assert_eq!(q.gone(), Some(queued("b")), "b shows once a is gone");
    q.closing();
    assert_eq!(q.gone(), None);
    assert_eq!(
        q.push(queued("c")),
        Some(queued("c")),
        "idle once b is gone"
    );
}

fn queued(uri: &str) -> QueuedLink {
    QueuedLink::new(uri.into())
}

#[test]
fn the_backlog_refuses_a_call_that_would_pass_the_cap_whole() {
    let backlog = Backlog::default();
    assert!(backlog.try_add(MAX_WAITING_LINKS - 4));
    assert!(!backlog.try_add(5));
    assert_eq!(
        backlog.waiting(),
        MAX_WAITING_LINKS - 4,
        "a refused call counts nothing"
    );
    assert!(backlog.try_add(4), "what still fits is accepted");
    assert_eq!(backlog.waiting(), MAX_WAITING_LINKS);
    assert!(!backlog.try_add(1));
}

#[test]
fn a_released_link_frees_its_room() {
    let backlog = Backlog::default();
    assert!(backlog.try_add(MAX_WAITING_LINKS));
    backlog.release(2);
    assert!(!backlog.try_add(3));
    assert!(backlog.try_add(2));
    assert_eq!(backlog.waiting(), MAX_WAITING_LINKS);
}

#[test]
fn links_nobody_can_turn_away_count_and_a_call_without_links_is_still_accepted() {
    let backlog = Backlog::default();
    backlog.add(MAX_WAITING_LINKS + 5);
    assert_eq!(backlog.waiting(), MAX_WAITING_LINKS + 5);
    assert!(backlog.try_add(0), "Activate carries no link");
    assert!(!backlog.try_add(1));
}

#[test]
fn releasing_more_than_was_counted_stops_at_zero() {
    let backlog = Backlog::default();
    backlog.add(2);
    backlog.release(5);
    assert_eq!(backlog.waiting(), 0);
    assert!(
        backlog.try_add(MAX_WAITING_LINKS),
        "a wrapped count would refuse every call"
    );
}

#[test]
fn a_clone_counts_with_the_original() {
    let backlog = Backlog::default();
    backlog.clone().add(3);
    assert_eq!(backlog.waiting(), 3);
}

#[test]
fn presenting_a_link_frees_its_room_and_waiting_does_not() {
    let backlog = Backlog::default();
    let mut q = LinkQueue::counting(backlog.clone());
    backlog.add(3);
    assert_eq!(q.push(queued("a")), Some(queued("a")));
    assert_eq!(backlog.waiting(), 2, "a is presented");
    assert_eq!(q.push(queued("b")), None);
    assert_eq!(q.push(queued("c")), None);
    assert_eq!(backlog.waiting(), 2, "b and c still wait");
    q.closing();
    assert_eq!(backlog.waiting(), 2, "closing presents nothing");
    assert_eq!(q.gone(), Some(queued("b")));
    assert_eq!(backlog.waiting(), 1);
    q.closing();
    assert_eq!(q.gone(), Some(queued("c")));
    assert_eq!(backlog.waiting(), 0);
    q.closing();
    assert_eq!(q.gone(), None);
    assert_eq!(backlog.waiting(), 0, "an empty queue frees nothing");
}

#[test]
fn discarding_the_waiting_links_frees_their_room() {
    let backlog = Backlog::default();
    let mut q = LinkQueue::counting(backlog.clone());
    backlog.add(3);
    q.push(queued("a"));
    q.push(queued("b"));
    q.push(queued("c"));
    q.discard_waiting();
    assert_eq!(backlog.waiting(), 0);
    assert_eq!(q.waiting(), 0);
    q.closing();
    assert_eq!(q.gone(), None, "nothing is left to present");
}

#[test]
fn clicks_and_menu_items() {
    let mut p = picker(3);
    assert_eq!(
        p.handle(Input::Click {
            index: 1,
            keep_open: true
        }),
        Outcome::Launch {
            index: 1,
            action: None,
            keep_open: true
        }
    );
    assert_eq!(
        p.handle(Input::Click {
            index: 9,
            keep_open: false
        }),
        Outcome::None
    );
    assert_eq!(p.handle(Input::OpenMenu(None)), Outcome::None);
    assert_eq!(p.menu, Some(1));
    assert_eq!(
        p.menu_items(),
        vec![
            MenuItem::Open,
            MenuItem::Action(TileAction::Private),
            MenuItem::OpenKeep,
            MenuItem::Copy
        ]
    );
    assert_eq!(
        p.handle(Input::ChooseItem {
            index: 1,
            item: MenuItem::Action(TileAction::Private),
            keep_open: false
        }),
        Outcome::Launch {
            index: 1,
            action: Some(TileAction::Private),
            keep_open: false
        }
    );
    assert_eq!(p.menu, None);
    assert_eq!(
        p.handle(Input::ChooseItem {
            index: 9,
            item: MenuItem::Open,
            keep_open: false
        }),
        Outcome::None,
        "no such tile"
    );
}

#[test]
fn keyboard_drives_the_actions_menu() {
    let mut p = menu_picker();
    assert_eq!((p.menu, p.menu_focus), (Some(1), 0));
    p.handle(Input::Move(0, 1));
    assert_eq!(
        (p.focus, p.menu_focus),
        (1, 1),
        "arrows move within the menu"
    );
    assert_eq!(
        p.handle(Input::Activate { ctrl: false }),
        Outcome::Launch {
            index: 1,
            action: Some(TileAction::Private),
            keep_open: false
        },
        "Private is listed before the desktop actions"
    );
    assert_eq!(p.menu, None);

    p.handle(Input::OpenMenu(None));
    assert_eq!(p.menu_focus, 0, "a reopened menu starts at its first row");
    for step in [Input::Move(0, 1), Input::Move(1, 0), Input::Move(0, 9)] {
        p.handle(step);
    }
    assert_eq!(p.menu_focus, 4, "clamped at the last row");
    p.handle(Input::Move(-1, -9));
    assert_eq!(p.menu_focus, 0, "clamped at the first row");
    p.handle(Input::DigitPress(1));
    assert_eq!(
        p.handle(Input::DigitRelease(1)),
        Outcome::None,
        "digits do not launch tiles under the menu"
    );

    p.handle(Input::Escape);
    assert_eq!(p.menu, None);
    assert_eq!(
        p.handle(Input::Activate { ctrl: false }),
        Outcome::Launch {
            index: 1,
            action: None,
            keep_open: false
        },
        "with the menu closed Enter launches the tile"
    );
}

#[test]
fn escape_closes_menu_first_then_cancels() {
    let mut p = picker(2);
    p.handle(Input::OpenMenu(Some(0)));
    assert_eq!(p.handle(Input::Escape), Outcome::None);
    assert_eq!(p.menu, None);
    assert_eq!(p.handle(Input::Escape), Outcome::Cancel);
}

#[test]
fn focus_loss_cancels_unless_keep_open() {
    let mut p = picker(2);
    assert_eq!(p.handle(Input::FocusLost), Outcome::Cancel);
    p.keep_open = true;
    assert_eq!(p.handle(Input::FocusLost), Outcome::None);
    assert_eq!(
        p.handle(Input::Escape),
        Outcome::Cancel,
        "Esc still closes a keep-open picker"
    );
}

#[test]
fn keep_open_gestures_emit_keep_open_launches_without_admitting_them() {
    let keep_open_launches = [
        Input::Activate { ctrl: true },
        Input::Click {
            index: 0,
            keep_open: true,
        },
        Input::ChooseItem {
            index: 0,
            item: MenuItem::OpenKeep,
            keep_open: false,
        },
    ];
    for input in keep_open_launches {
        let mut p = picker(2);
        assert!(matches!(
            p.handle(input.clone()),
            Outcome::Launch {
                keep_open: true,
                ..
            }
        ));
        // The app sets keep-open only once it admits the launch (app::admit).
        assert!(!p.keep_open, "{input:?}");
    }
    let mut p = picker(2);
    p.handle(Input::PressStart(1));
    assert!(matches!(
        p.handle(Input::PressEnd {
            index: 1,
            keep_open: true
        }),
        Outcome::Launch {
            keep_open: true,
            ..
        }
    ));
    assert!(!p.keep_open, "pointer release");
}

#[test]
fn copy_and_empty_picker() {
    let mut p = picker(0);
    assert_eq!(
        p.handle(Input::Copy),
        Outcome::Copy {
            uri: URI.into(),
            seq: 1
        }
    );
    assert_eq!(p.handle(Input::Activate { ctrl: false }), Outcome::None);
    assert_eq!(p.handle(Input::Move(1, 1)), Outcome::None);
}

#[test]
fn key_mapping() {
    assert_eq!(
        map_key(Key::Enter, true, true),
        Some(Input::Activate { ctrl: true })
    );
    assert_eq!(map_key(Key::Enter, false, false), None);
    assert_eq!(
        map_key(Key::Digit(4), false, true),
        Some(Input::DigitPress(4))
    );
    assert_eq!(
        map_key(Key::Digit(4), false, false),
        Some(Input::DigitRelease(4))
    );
    assert_eq!(map_key(Key::CopyC, true, true), Some(Input::Copy));
    assert_eq!(map_key(Key::CopyC, false, true), None);
    assert_eq!(map_key(Key::Up, false, true), Some(Input::Move(0, -1)));
    assert_eq!(map_key(Key::Menu, false, true), Some(Input::OpenMenu(None)));
    assert_eq!(map_key(Key::Escape, false, true), Some(Input::Escape));
}

#[test]
fn a_link_sent_twice_at_once_is_one_link() {
    let backlog = Backlog::default();
    let mut q = LinkQueue::counting(backlog.clone());
    let start = std::time::Instant::now();
    backlog.add(2);
    assert_eq!(q.arrive(queued("a"), start), Some(queued("a")));
    assert_eq!(
        q.arrive(queued("a"), start + DUPLICATE_WINDOW / 2),
        None,
        "the repeat is dropped"
    );
    assert_eq!(q.waiting(), 0, "nothing waits behind the first");
    assert_eq!(backlog.waiting(), 0, "the repeat gives its room back");
}

#[test]
fn the_same_link_later_or_another_link_at_once_still_waits() {
    let mut q = LinkQueue::default();
    let start = std::time::Instant::now();
    assert_eq!(q.arrive(queued("a"), start), Some(queued("a")));
    assert_eq!(q.arrive(queued("b"), start), None, "another link waits");
    let later = start + DUPLICATE_WINDOW;
    assert_eq!(
        q.arrive(queued("a"), later),
        None,
        "a, once the window is over, waits"
    );
    assert_eq!(q.arrive(queued("b"), later), None, "so does b");
    assert_eq!(q.waiting(), 3);
}

#[test]
fn a_repeat_of_the_shown_or_the_last_waiting_link_is_dropped() {
    let backlog = Backlog::default();
    let mut q = LinkQueue::counting(backlog.clone());
    let start = std::time::Instant::now();
    let soon = start + DUPLICATE_WINDOW / 2;
    backlog.add(6);
    assert_eq!(q.arrive(queued("a"), start), Some(queued("a")));
    assert_eq!(q.arrive(queued("b"), start), None);
    assert_eq!(q.arrive(queued("c"), start), None);
    assert_eq!(
        q.arrive(queued("a"), soon),
        None,
        "a repeats the shown link"
    );
    assert_eq!(
        q.arrive(queued("c"), soon),
        None,
        "c repeats the last waiting"
    );
    assert_eq!(backlog.waiting(), 3, "the repeats gave their room back");
    assert_eq!(
        q.arrive(queued("b"), soon),
        None,
        "b, behind c, is a link sent again"
    );
    assert_eq!(q.waiting(), 3, "b waits a second time");
    assert_eq!(backlog.waiting(), 3, "and keeps its room");
}

#[test]
fn a_link_sent_again_once_its_picker_closes_is_presented() {
    let mut q = LinkQueue::default();
    let start = std::time::Instant::now();
    let soon = start + DUPLICATE_WINDOW / 4;
    assert_eq!(q.arrive(queued("a"), start), Some(queued("a")));
    q.closing();
    assert_eq!(
        q.arrive(queued("a"), soon),
        None,
        "it waits for the closing picker"
    );
    assert_eq!(q.waiting(), 1, "and is not taken for a repeat");
    assert_eq!(q.gone(), Some(queued("a")));
    q.closing();
    assert_eq!(q.gone(), None);
    assert_eq!(
        q.arrive(queued("a"), soon),
        Some(queued("a")),
        "presented on an idle queue too"
    );
}

#[test]
fn a_link_coming_back_after_a_failure_is_never_taken_for_a_repeat() {
    let mut q = LinkQueue::default();
    let start = std::time::Instant::now();
    assert_eq!(q.arrive(queued("a"), start), Some(queued("a")));
    let back = QueuedLink {
        error: Some("the app quit".into()),
        ..queued("a")
    };
    q.closing();
    assert_eq!(
        q.push(back.clone()),
        None,
        "it waits for the closing picker"
    );
    assert_eq!(q.gone(), Some(back));
    assert_eq!(
        q.arrive(queued("a"), start),
        None,
        "a sent at once waits behind it"
    );
    assert_eq!(q.waiting(), 1);
}
