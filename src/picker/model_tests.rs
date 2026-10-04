use super::tests::{URI, apps, menu_picker, new_window, picker, tile, tiles};
use super::*;

fn launch(index: usize, action: Option<TileAction>, keep_open: bool) -> Outcome {
    Outcome::Launch {
        index,
        action,
        keep_open,
    }
}

#[test]
fn the_menu_lists_open_then_private_then_desktop_actions_then_keep_and_copy() {
    let other = TileAction::Desktop {
        id: "other".into(),
        label: "Other".into(),
    };
    let mut ts = tiles(1, false);
    ts[0].actions = vec![new_window(), other.clone(), TileAction::Private];
    let mut p = Picker::new(URI.into(), ts);
    assert!(p.menu_items().is_empty(), "no menu, no rows");
    p.handle(Input::OpenMenu(None));
    assert_eq!(
        p.menu_items(),
        vec![
            MenuItem::Open,
            MenuItem::Action(TileAction::Private),
            MenuItem::Action(new_window()),
            MenuItem::Action(other),
            MenuItem::OpenKeep,
            MenuItem::Copy,
        ],
        "Private first although the tile lists it last"
    );
}

#[test]
fn every_tile_has_a_menu_even_without_actions() {
    let mut p = Picker::new(URI.into(), tiles(2, false));
    p.handle(Input::OpenMenu(Some(1)));
    assert_eq!(p.menu, Some(1));
    assert_eq!(
        p.menu_items(),
        vec![MenuItem::Open, MenuItem::OpenKeep, MenuItem::Copy]
    );
}

#[test]
fn menu_items_carry_shortcuts_and_the_separator_sits_before_copy() {
    let rows = [
        (MenuItem::Open, &["Enter"][..], false),
        (MenuItem::Action(TileAction::Private), &[][..], false),
        (MenuItem::Action(new_window()), &[][..], false),
        (MenuItem::OpenKeep, &["Ctrl", "Enter"][..], false),
        (MenuItem::Copy, &["Ctrl", "C"][..], true),
    ];
    for (item, shortcut, separator) in rows {
        assert_eq!(item.shortcut(), shortcut, "{item:?}");
        assert_eq!(item.separator_before(), separator, "{item:?}");
    }
}

#[test]
fn the_separator_is_never_a_focus_stop() {
    let mut p = menu_picker();
    let items = p.menu_items();
    for expected in 1..items.len() {
        p.handle(Input::Move(0, 1));
        assert_eq!(p.menu_focus, expected, "one arrow, one row");
    }
    assert_eq!(items.get(p.menu_focus), Some(&MenuItem::Copy));
}

#[test]
fn enter_activates_the_focused_row_of_each_kind() {
    let cases = [
        (0, launch(1, None, false)),
        (1, launch(1, Some(TileAction::Private), false)),
        (2, launch(1, Some(new_window()), false)),
        (3, launch(1, None, true)),
    ];
    for (row, expected) in cases {
        let mut p = menu_picker();
        p.handle(Input::Move(0, row));
        assert_eq!(
            p.handle(Input::Activate { ctrl: false }),
            expected,
            "row {row}"
        );
        assert_eq!(p.menu, None, "row {row}");
    }
    let mut p = menu_picker();
    p.handle(Input::Move(0, 4));
    assert_eq!(
        p.handle(Input::Activate { ctrl: false }),
        Outcome::Copy {
            uri: URI.into(),
            seq: 1
        }
    );
    assert_eq!(p.menu, None, "Copy closes the menu");
}

#[test]
fn ctrl_enter_is_open_and_keep_whatever_row_is_focused() {
    for row in 0..5 {
        let mut p = menu_picker();
        p.handle(Input::Move(0, row));
        assert_eq!(
            p.handle(Input::Activate { ctrl: true }),
            launch(1, None, true),
            "row {row}"
        );
        assert_eq!(p.menu, None, "row {row}");
    }
}

#[test]
fn ctrl_c_copies_and_closes_the_menu() {
    let mut p = menu_picker();
    assert_eq!(
        p.handle(map_key(Key::CopyC, true, true).unwrap()),
        Outcome::Copy {
            uri: URI.into(),
            seq: 1
        }
    );
    assert_eq!(p.menu, None);
}

#[test]
fn clicking_a_menu_row_does_what_that_row_says() {
    let cases = [
        (MenuItem::Open, launch(1, None, false)),
        (
            MenuItem::Action(new_window()),
            launch(1, Some(new_window()), false),
        ),
        (MenuItem::OpenKeep, launch(1, None, true)),
        (
            MenuItem::Copy,
            Outcome::Copy {
                uri: URI.into(),
                seq: 1,
            },
        ),
    ];
    for (item, expected) in cases {
        let mut p = menu_picker();
        assert_eq!(
            p.handle(Input::ChooseItem {
                index: 1,
                item: item.clone(),
                keep_open: false
            }),
            expected,
            "{item:?}"
        );
        assert_eq!(p.menu, None, "{item:?}");
    }
}

#[test]
fn ctrl_clicking_a_menu_row_keeps_the_picker_open() {
    let cases = [
        (MenuItem::Open, launch(1, None, true)),
        (
            MenuItem::Action(TileAction::Private),
            launch(1, Some(TileAction::Private), true),
        ),
        (
            MenuItem::Action(new_window()),
            launch(1, Some(new_window()), true),
        ),
        (MenuItem::OpenKeep, launch(1, None, true)),
        (
            MenuItem::Copy,
            Outcome::Copy {
                uri: URI.into(),
                seq: 1,
            },
        ),
    ];
    for (item, expected) in cases {
        let mut p = menu_picker();
        assert!(!p.pinned, "unpinned: only the click's Ctrl keeps it open");
        assert_eq!(
            p.handle(Input::ChooseItem {
                index: 1,
                item: item.clone(),
                keep_open: true
            }),
            expected,
            "{item:?}"
        );
    }
}

#[test]
fn the_click_modifier_adds_to_a_keep_open_request_of_every_click_gesture() {
    let click = |keep_open| Input::Click {
        index: 1,
        keep_open,
    };
    let press_end = |keep_open| Input::PressEnd {
        index: 1,
        keep_open,
    };
    let choose = |keep_open| Input::ChooseItem {
        index: 1,
        item: MenuItem::Open,
        keep_open,
    };
    for gesture in [click, press_end, choose] {
        assert_eq!(gesture(false).with_ctrl(true), gesture(true));
        assert_eq!(gesture(true).with_ctrl(false), gesture(true));
        assert_eq!(gesture(false).with_ctrl(false), gesture(false));
    }
    for other in [
        Input::Activate { ctrl: false },
        Input::Copy,
        Input::CloseMenu,
    ] {
        assert_eq!(other.clone().with_ctrl(true), other, "not a click");
    }
}

#[test]
fn with_the_menu_open_a_press_or_click_on_a_tile_only_closes_it() {
    let mut p = picker(3);
    p.handle(Input::OpenMenu(Some(0)));
    let Outcome::ArmLongPress(_) = p.handle(Input::PressStart(1)) else {
        panic!("must arm")
    };
    assert_eq!(p.menu, None, "the press closes the menu");
    assert_eq!(
        p.handle(Input::PressEnd {
            index: 1,
            keep_open: false
        }),
        Outcome::None,
        "its release launches nothing"
    );
    p.handle(Input::PressStart(1));
    assert_eq!(
        p.handle(Input::PressEnd {
            index: 1,
            keep_open: false
        }),
        launch(1, None, false),
        "the next press is an ordinary click"
    );

    p.handle(Input::OpenMenu(Some(0)));
    assert_eq!(
        p.handle(Input::Click {
            index: 1,
            keep_open: true
        }),
        Outcome::None,
        "middle-click"
    );
    assert_eq!(p.menu, None);
    assert_eq!(
        p.handle(Input::Click {
            index: 1,
            keep_open: true
        }),
        launch(1, None, true),
        "and again with the menu closed"
    );
}

#[test]
fn a_long_press_or_right_click_on_another_tile_opens_its_menu() {
    let mut p = picker(3);
    p.handle(Input::OpenMenu(Some(0)));
    p.handle(Input::Move(0, 1));
    p.handle(Input::OpenMenu(Some(2)));
    assert_eq!((p.menu, p.focus, p.menu_focus), (Some(2), 2, 0));

    let Outcome::ArmLongPress(seq) = p.handle(Input::PressStart(1)) else {
        panic!("must arm")
    };
    assert_eq!(p.handle(Input::LongPress(seq)), Outcome::None);
    assert_eq!((p.menu, p.focus, p.menu_focus), (Some(1), 1, 0));
    assert_eq!(
        p.handle(Input::PressEnd {
            index: 1,
            keep_open: false
        }),
        Outcome::None,
        "release after a long-press never launches"
    );
}

#[test]
fn pin_makes_every_launch_path_keep_the_picker_open() {
    let gestures = [
        ("digit", vec![Input::DigitPress(2), Input::DigitRelease(2)]),
        ("enter", vec![Input::Activate { ctrl: false }]),
        (
            "click",
            vec![Input::Click {
                index: 1,
                keep_open: false,
            }],
        ),
        (
            "press release",
            vec![
                Input::PressStart(1),
                Input::PressEnd {
                    index: 1,
                    keep_open: false,
                },
            ],
        ),
        (
            "menu row click",
            vec![
                Input::OpenMenu(Some(1)),
                Input::ChooseItem {
                    index: 1,
                    item: MenuItem::Open,
                    keep_open: false,
                },
            ],
        ),
        (
            "menu action click",
            vec![
                Input::OpenMenu(Some(1)),
                Input::ChooseItem {
                    index: 1,
                    item: MenuItem::Action(TileAction::Private),
                    keep_open: false,
                },
            ],
        ),
        (
            "menu Enter",
            vec![Input::OpenMenu(Some(1)), Input::Activate { ctrl: false }],
        ),
    ];
    for pinned in [true, false] {
        for (name, inputs) in gestures.clone() {
            let mut p = picker(3);
            if pinned {
                p.handle(Input::TogglePin);
            }
            let last = inputs.into_iter().map(|input| p.handle(input)).last();
            assert!(
                matches!(last, Some(Outcome::Launch { keep_open, .. }) if keep_open == pinned),
                "{name}, pinned {pinned}: {last:?}"
            );
        }
    }
}

#[test]
fn a_pinned_picker_survives_focus_loss_and_unpinning_restores_closing() {
    let mut p = picker(2);
    p.handle(Input::TogglePin);
    assert!(p.pinned);
    assert_eq!(p.handle(Input::FocusLost), Outcome::None);
    p.handle(Input::TogglePin);
    assert!(!p.pinned);
    assert_eq!(p.handle(Input::FocusLost), Outcome::Cancel);
}

#[test]
fn unpinning_clears_the_keep_open_latch_a_pinned_launch_set() {
    let mut p = picker(2);
    p.handle(Input::TogglePin);
    let Outcome::Launch { keep_open, .. } = p.handle(Input::Activate { ctrl: false }) else {
        panic!("must launch")
    };
    p.keep_open |= keep_open; // what `app::admit` does for an admitted launch
    assert!(p.keep_open, "the pinned launch latched keep-open");
    p.handle(Input::TogglePin);
    assert!(!p.keep_open);
    assert_eq!(p.handle(Input::FocusLost), Outcome::Cancel);
}

#[test]
fn a_launch_requested_before_pinning_still_settles_open() {
    let mut p = picker(1);
    assert!(!p.keeps_open(false));
    assert!(p.keeps_open(true));
    p.handle(Input::TogglePin);
    assert!(p.keeps_open(false));
}

#[test]
fn skipping_to_the_next_link_closes_the_picker_even_with_the_menu_open_or_pinned() {
    let mut p = picker(2);
    p.handle(Input::TogglePin);
    p.handle(Input::OpenMenu(Some(0)));
    assert_eq!(p.handle(Input::SkipToNext), Outcome::Cancel);
}

#[test]
fn peek_shows_the_next_link_without_taking_it() {
    let link = |u: &str| QueuedLink {
        uri: u.into(),
        error: None,
        failed: None,
    };
    let mut q = LinkQueue::default();
    assert_eq!(q.peek(), None, "idle");
    q.push(link("a"));
    assert_eq!(q.peek(), None, "the presented link is not waiting");
    q.push(link("b"));
    q.push(link("c"));
    assert_eq!(q.peek(), Some(&link("b")));
    assert_eq!(q.waiting(), 2, "peeking takes nothing");
    q.closing();
    q.gone();
    assert_eq!(q.peek(), Some(&link("c")));
    q.closing();
    q.gone();
    assert_eq!(q.peek(), None);
}

#[test]
fn a_failure_names_its_tile_and_clears_with_the_next_attempt() {
    let mut p = Picker::new(URI.into(), apps(&[("chrome", 3), ("brave", 1)]));
    p.fail("boom".into(), Some(Target::from(&p.tiles[1])));
    assert_eq!(p.error.as_deref(), Some("boom"));
    assert_eq!(
        p.failed,
        Some(Target {
            app_id: "chrome".into(),
            profile_key: Some("p1".into()),
            label: "p1".into(),
        })
    );
    p.handle(Input::ToggleDetails);
    assert!(p.details_open);
    p.handle(Input::ToggleDetails);
    assert!(!p.details_open, "Show details toggles");
    p.handle(Input::ToggleDetails);
    p.clear_failure();
    assert_eq!((p.error, p.failed, p.details_open), (None, None, false));
}

#[test]
fn a_failure_returns_the_focus_to_its_tile() {
    let ts = apps(&[("chrome", 3), ("brave", 1)]);
    let mut p = Picker::new(URI.into(), ts.clone());
    p.handle(Input::Move(1, 0));
    let Outcome::Launch { index, .. } = p.handle(Input::Activate { ctrl: false }) else {
        panic!("must launch")
    };
    p.handle(Input::Move(2, 0));
    assert_eq!(p.focus, 3, "focus moved while the launch was pending");
    p.fail("boom".into(), Some(Target::from(&ts[index])));
    assert_eq!(p.focus, index, "Enter retries the tile that failed");
}

#[test]
fn a_failure_of_a_tile_that_is_gone_leaves_the_focus_alone() {
    let mut p = Picker::new(URI.into(), apps(&[("chrome", 3)]));
    p.focus = 2;
    let gone = Target {
        app_id: "brave".into(),
        profile_key: None,
        label: "Brave".into(),
    };
    p.fail("boom".into(), Some(gone));
    assert_eq!(p.focus, 2);
    p.fail("boom".into(), None);
    assert_eq!(p.focus, 2);
}

/// A launch of tile 1 is pending when the user opens the menu of tile 3 and arrows to its second row.
fn pending_launch_with_another_menu_open() -> (Picker, Vec<Tile>) {
    let ts = apps(&[("chrome", 3), ("brave", 1)]);
    let mut p = Picker::new(URI.into(), ts.clone());
    p.handle(Input::Move(1, 0));
    p.handle(Input::Activate { ctrl: false });
    p.handle(Input::OpenMenu(Some(3)));
    p.handle(Input::Move(0, 1));
    (p, ts)
}

fn assert_failure_left_the_menu_alone(p: &mut Picker, failed: &Tile) {
    assert_eq!(p.error.as_deref(), Some("boom"));
    assert_eq!(p.failed, Some(Target::from(failed)), "the banner names it");
    assert_eq!((p.menu, p.focus), (Some(3), 3), "the menu keeps the focus");
    assert_eq!(
        p.handle(Input::Activate { ctrl: false }),
        launch(3, Some(TileAction::Private), false),
        "Enter acts on the menu's focused row"
    );
}

#[test]
fn a_failure_leaves_an_open_menu_and_its_focus_alone() {
    let (mut p, ts) = pending_launch_with_another_menu_open();
    assert_eq!(p.settle(1, false, Some("boom".into())), Settled::Failed);
    assert_failure_left_the_menu_alone(&mut p, &ts[1]);
}

#[test]
fn an_early_failure_leaves_an_open_menu_and_its_focus_alone() {
    let (mut p, ts) = pending_launch_with_another_menu_open();
    let failure = LaunchFailure {
        message: "boom".into(),
        ..early(&ts[1])
    };
    assert_eq!(early_failure(Some(&mut p), failure), None);
    assert_failure_left_the_menu_alone(&mut p, &ts[1]);
}

#[test]
fn closing_the_menu_keeps_the_focus_and_the_next_failure_refocuses_its_tile() {
    let (mut p, ts) = pending_launch_with_another_menu_open();
    p.settle(1, false, Some("boom".into()));
    p.handle(Input::Escape);
    assert_eq!(p.focus, 3, "closing the menu does not move the focus");
    p.settle(1, false, Some("again".into()));
    assert_eq!((p.menu, p.focus), (None, 1));
    assert_eq!(p.failed, Some(Target::from(&ts[1])));
}

#[test]
fn no_failure_path_leaves_the_focus_off_the_open_menu() {
    let paths: [fn(&mut Picker, usize); 3] = [
        |p, i| p.fail("boom".into(), Some(Target::from(&p.tiles[i]))),
        |p, i| {
            p.settle(i, false, Some("boom".into()));
        },
        |p, i| {
            let failure = early(&p.tiles[i]);
            assert_eq!(early_failure(Some(p), failure), None);
        },
    ];
    for (n, fail) in paths.into_iter().enumerate() {
        for open in [None, Some(3)] {
            let mut p = Picker::new(URI.into(), apps(&[("chrome", 3), ("brave", 1)]));
            if open.is_some() {
                p.handle(Input::OpenMenu(open));
            }
            fail(&mut p, 1);
            assert_eq!(p.menu, open, "path {n} leaves the menu as it was");
            assert!(
                p.menu.is_none_or(|menu| menu == p.focus),
                "path {n}, menu {open:?}: focus {}",
                p.focus
            );
        }
    }
}

#[test]
fn a_target_matches_its_app_and_profile_only() {
    let ts = apps(&[("chrome", 2), ("brave", 1)]);
    let target = Target::from(&ts[1]);
    assert!(target.matches(&ts[1]));
    assert!(!target.matches(&ts[0]), "same app, other profile");
    assert!(
        !target.matches(&tile("brave", Some("p1"))),
        "same profile key, other app"
    );
    let plain = tile("firefox", None);
    assert_eq!(Target::from(&plain).profile_key, None);
    assert!(Target::from(&plain).matches(&plain));
}

#[test]
fn failure_identity_survives_the_queue_and_refocuses_the_failed_tile() {
    let ts = apps(&[("chrome", 3), ("brave", 1)]);
    let failed = Some(Target::from(&ts[2]));
    let requeued = early_failure(None, early(&ts[2])).expect("a closed picker re-queues the link");
    let mut q = LinkQueue::default();
    q.push(QueuedLink {
        uri: "https://other/".into(),
        error: None,
        failed: None,
    });
    q.push(requeued.clone());
    q.closing();
    assert_eq!(q.gone(), Some(requeued.clone()));

    let reopened = Picker::from_link(requeued.clone(), ts.clone());
    assert_eq!(reopened.focus, 2, "the failed profile has focus again");
    assert_eq!(reopened.error.as_deref(), Some("exited early"));
    assert_eq!(reopened.failed, failed);

    let reordered: Vec<Tile> = ts.iter().rev().cloned().collect();
    assert_eq!(
        Picker::from_link(requeued.clone(), reordered).focus,
        1,
        "found by identity, not by position"
    );
    let gone = Picker::from_link(requeued, apps(&[("brave", 1)]));
    assert_eq!(gone.focus, 0, "the tile is gone: default focus");
    assert_eq!(gone.failed, failed, "its name still fills the banner");
}

fn early(tile: &Tile) -> LaunchFailure {
    LaunchFailure {
        uri: URI.into(),
        failed: Target::from(tile),
        message: "exited early".into(),
    }
}

#[test]
fn an_early_failure_of_an_open_picker_updates_it_in_place() {
    let ts = apps(&[("chrome", 3), ("brave", 1)]);
    let mut p = Picker::new(URI.into(), ts.clone());
    p.pinned = true;
    p.focus = 3;
    assert_eq!(early_failure(Some(&mut p), early(&ts[1])), None);
    assert_eq!(p.error.as_deref(), Some("exited early"));
    assert_eq!(p.failed, Some(Target::from(&ts[1])));
    assert_eq!(p.focus, 1, "Enter retries the profile that failed");
}

#[test]
fn an_early_failure_of_a_closed_picker_comes_back_carrying_the_failed_tile() {
    let ts = apps(&[("chrome", 3)]);
    assert_eq!(
        early_failure(None, early(&ts[2])),
        Some(QueuedLink {
            uri: URI.into(),
            error: Some("exited early".into()),
            failed: Some(Target::from(&ts[2])),
        })
    );
}

#[test]
fn a_failed_launch_records_its_tile_focuses_it_and_does_not_relax_the_focus() {
    let mut p = Picker::new(URI.into(), apps(&[("chrome", 3), ("brave", 1)]));
    p.pinned = true;
    p.focus = 3;
    assert_eq!(p.settle(1, true, Some("boom".into())), Settled::Failed);
    assert_eq!(p.error.as_deref(), Some("boom"));
    assert_eq!(p.failed, Some(Target::from(&p.tiles[1])));
    assert_eq!(p.focus, 1);
    assert_eq!(
        p.settle(1, true, None),
        Settled::KeepOpen { relax_focus: true },
        "a failure did not count as the first kept-open launch"
    );
}

#[test]
fn a_pinned_launch_that_succeeds_keeps_the_picker_and_relaxes_the_focus_once() {
    let mut p = picker(2);
    p.handle(Input::TogglePin);
    assert_eq!(
        p.settle(0, false, None),
        Settled::KeepOpen { relax_focus: true }
    );
    assert_eq!(
        p.settle(1, false, None),
        Settled::KeepOpen { relax_focus: false },
        "already on demand"
    );
}

#[test]
fn an_unpinned_launch_that_succeeds_closes_unless_it_asked_to_stay() {
    let mut p = picker(2);
    assert_eq!(p.settle(0, false, None), Settled::Close);
    assert_eq!(
        p.settle(0, true, None),
        Settled::KeepOpen { relax_focus: true }
    );
}

#[test]
fn a_launch_that_succeeds_dismisses_the_failure_before_it() {
    let mut p = picker(2);
    p.fail("boom".into(), Some(Target::from(&p.tiles[0])));
    p.handle(Input::ToggleDetails);
    p.settle(0, true, None);
    assert_eq!((p.error, p.failed, p.details_open), (None, None, false));
}

#[test]
fn a_link_without_a_failure_opens_on_the_first_tile() {
    let link = QueuedLink {
        uri: URI.into(),
        error: None,
        failed: None,
    };
    let p = Picker::from_link(link, tiles(3, true));
    assert_eq!((p.focus, p.error, p.failed), (0, None, None));
    assert_eq!(p.uri, URI);
}

#[test]
fn copying_flashes_until_its_own_expiry() {
    let mut p = picker(1);
    assert_eq!(p.copied, None);
    let Outcome::Copy { seq: first, .. } = p.handle(Input::Copy) else {
        panic!("must copy")
    };
    assert_eq!(p.copied, Some(first));
    let Outcome::Copy { seq: second, .. } = p.handle(Input::Copy) else {
        panic!("must copy")
    };
    assert_ne!(first, second);
    p.handle(Input::CopiedExpired(first));
    assert_eq!(p.copied, Some(second), "the first flash's timer is stale");
    p.handle(Input::CopiedExpired(second));
    assert_eq!(p.copied, None);
}

fn plain(apps: &[&str]) -> Vec<Tile> {
    apps.iter().map(|app| tile(app, None)).collect()
}

/// Rows: `[0 1 2 3] [4 5]`, profile tiles and plain tiles of three apps in a row.
fn across_apps() -> Vec<Tile> {
    let mut ts = apps(&[("chrome", 3)]);
    ts.extend(plain(&["brave-origin", "firefox"]));
    ts.push(tile("chrome", Some("p3")));
    ts
}

#[test]
fn tiles_fill_each_row_in_order_whatever_app_they_belong_to() {
    assert_eq!(
        rows(&across_apps()),
        vec![vec![0, 1, 2, 3], vec![4, 5]],
        "no break where the app changes"
    );
    assert_eq!(
        rows(&apps(&[("chrome", 3), ("brave", 1)])),
        vec![vec![0, 1, 2, 3]]
    );
    assert_eq!(
        rows(&apps(&[("a", 5), ("b", 1), ("c", 4)])),
        vec![vec![0, 1, 2, 3], vec![4, 5, 6, 7], vec![8, 9]]
    );
    assert_eq!(
        rows(&apps(&[("a", 1), ("b", 1), ("a", 1)])),
        vec![vec![0, 1, 2]]
    );
    assert_eq!(
        rows(&plain(&["a", "b", "c", "d", "e"])),
        vec![vec![0, 1, 2, 3], vec![4]]
    );
    assert_eq!(rows(&plain(&["a", "b"])), vec![vec![0, 1]]);
    assert_eq!(rows(&[]), Vec::<Vec<usize>>::new());
}

#[test]
fn arrows_follow_the_packed_rows() {
    let mut p = Picker::new(URI.into(), across_apps());
    let focus_after = |p: &mut Picker, dx, dy| {
        p.handle(Input::Move(dx, dy));
        p.focus
    };
    p.focus = 2;
    assert_eq!(
        focus_after(&mut p, 0, 1),
        5,
        "column 2 clamps to the two-tile row"
    );
    assert_eq!(
        focus_after(&mut p, 1, 0),
        5,
        "the row ends at its last tile"
    );
    assert_eq!(focus_after(&mut p, -1, 0), 4);
    assert_eq!(
        focus_after(&mut p, -1, 0),
        4,
        "no tile left of the row start"
    );
    assert_eq!(focus_after(&mut p, 0, -1), 0, "up keeps column 0");
    assert_eq!(focus_after(&mut p, 1, 0), 1);
    assert_eq!(focus_after(&mut p, 0, 1), 5, "column 1 of the short row");
    assert_eq!(focus_after(&mut p, 0, 1), 5, "no row below");

    let mut p = Picker::new(URI.into(), apps(&[("chrome", 6)]));
    p.handle(Input::Move(3, 0));
    p.handle(Input::Move(0, 1));
    assert_eq!(p.focus, 5, "column 3 clamps to the two-tile row");
}

#[test]
fn digits_still_pick_the_flat_index_across_groups() {
    let mut p = Picker::new(URI.into(), apps(&[("chrome", 3), ("brave", 1)]));
    p.handle(Input::DigitPress(4));
    assert_eq!(p.handle(Input::DigitRelease(4)), launch(3, None, false));
}

#[test]
fn keycaps_number_the_first_nine_tiles() {
    assert_eq!(keycap(0), Some(1));
    assert_eq!(keycap(8), Some(9));
    assert_eq!(keycap(9), None);
    assert_eq!(keycap(usize::MAX), None);
}
