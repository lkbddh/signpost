use std::path::PathBuf;

use keyboard::key::Named;

use super::launching::spawn_on_host;
use super::*;
use crate::picker::{Key, Settled, Target};
use crate::profiles::Tile;

#[test]
fn a_link_that_is_not_opened_is_logged_by_its_scheme_never_in_full() {
    let input = ["mailto:someone@example.org?subject=private", "https://ok/"].map(String::from);
    let mut kept = Vec::new();
    let log = crate::test_support::logs::logged(|| kept = valid_link_uris(&input));
    assert_eq!(kept, ["https://ok/"]);
    assert!(log.contains("mailto"), "{log}");
    assert!(!log.contains("someone@example.org"), "{log}");
}

/// An Exec can carry secrets of its own (`env` assignments, other links), so a launch is logged by its app and the
/// shape of its arguments, never by them.
#[test]
fn a_host_launch_logs_its_app_never_its_arguments() {
    let link = "https://example.org/account?token=private";
    let spec = crate::launch::spawn::LaunchSpec {
        argv: [
            "env",
            "API_KEY=secret",
            "BROWSER=https://private.example/",
            "firefox",
            "--new-window",
            link,
        ]
        .map(String::from)
        .to_vec(),
        cwd: None,
        token: Some("activation".into()),
    };
    let mut entry = crate::index::AppEntry {
        id: "firefox".into(),
        name: "Firefox".into(),
        icon: None,
        exec: None,
        exec_error: None,
        try_exec: None,
        work_dir: None,
        terminal: false,
        dbus_activatable: false,
        no_display: false,
        hidden: false,
        mime_types: Vec::new(),
        actions: Vec::new(),
        file: PathBuf::from("/usr/share/applications/firefox.desktop"),
    };
    entry.exec = Some(vec!["firefox".into(), "%u".into()]);
    let failed = Target {
        app_id: "firefox".into(),
        profile_key: None,
        label: "Firefox".into(),
    };
    let mut host = crate::host::HostEnv::parse(b"HOME=/home/u\0", std::path::Path::new("/home/u"));
    host.scopes = true;
    let log = crate::test_support::logs::logged(|| {
        drop(spawn_on_host(
            window::Id::unique(),
            1,
            link.into(),
            failed,
            spec,
            &entry,
            &host,
        ));
    });
    assert!(
        log.contains("app=firefox") && log.contains("args=6") && log.contains("token=true"),
        "{log}"
    );
    for private in [
        "secret",
        "private.example",
        "token=private",
        "activation",
        "--new-window",
    ] {
        assert!(!log.contains(private), "{private}: {log}");
    }
}

#[test]
fn link_uri_validation() {
    let input: Vec<String> = [
        "https://a/ b?x=%20#f",
        "http://b/",
        "mailto:x@y",
        "file:///etc",
        "not a url",
        "",
    ]
    .map(String::from)
    .to_vec();
    assert_eq!(
        valid_link_uris(&input),
        ["https://a/ b?x=%20#f", "http://b/"]
    );
}

#[test]
fn lookup_scheme_comes_from_the_parsed_url() {
    assert_eq!(
        link_scheme("HTTPS://Example.org/").as_deref(),
        Some("https")
    );
    assert_eq!(link_scheme(" https://a/").as_deref(), Some("https"));
    assert_eq!(link_scheme("Http://b/").as_deref(), Some("http"));
    assert_eq!(link_scheme("mailto:x@y"), None);
    assert_eq!(link_scheme("not a url"), None);
    let input: Vec<String> = ["HTTPS://Example.org/", " https://a/"]
        .map(String::from)
        .to_vec();
    assert_eq!(
        valid_link_uris(&input),
        input,
        "accepted exactly when a lookup scheme exists, and launched unchanged"
    );
}

fn key_event(pressed: bool) -> Event {
    let key = keyboard::Key::Named(Named::Enter);
    let physical_key = keyboard::key::Physical::Code(keyboard::key::Code::Enter);
    let location = keyboard::Location::Standard;
    let modifiers = keyboard::Modifiers::empty();
    Event::Keyboard(if pressed {
        keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key,
            location,
            modifiers,
            text: None,
            repeat: false,
        }
    } else {
        keyboard::Event::KeyReleased {
            key: key.clone(),
            modified_key: key,
            physical_key,
            location,
            modifiers,
        }
    })
}

#[test]
fn keys_a_widget_captured_never_reach_the_picker() {
    use cosmic::iced::event::Status::{Captured, Ignored};
    assert!(
        !forward_event(&key_event(true), Captured),
        "Enter on a focused button"
    );
    assert!(!forward_event(&key_event(false), Captured));
    assert!(forward_event(&key_event(true), Ignored));
    assert!(forward_event(&key_event(false), Ignored));
    let ctrl = Event::Keyboard(keyboard::Event::ModifiersChanged(keyboard::Modifiers::CTRL));
    assert!(
        forward_event(&ctrl, Captured),
        "Ctrl tracking sees every modifier change"
    );
    let finger = Event::Touch(cosmic::iced::touch::Event::FingerMoved {
        id: cosmic::iced::touch::Finger(1),
        position: cosmic::iced::Point::new(1.0, 1.0),
    });
    assert!(
        forward_event(&finger, Captured),
        "touch cancellation sees every move"
    );
    let cursor = Event::Mouse(cosmic::iced::mouse::Event::CursorMoved {
        position: cosmic::iced::Point::new(1.0, 1.0),
    });
    assert!(!forward_event(&cursor, Ignored), "no idle pointer traffic");
}

#[test]
fn a_window_opening_reaches_the_app() {
    let opened = Event::Window(window::Event::Opened {
        position: None,
        size: cosmic::iced::Size::new(1.0, 1.0),
    });
    assert!(forward_event(&opened, event::Status::Ignored));
}

#[test]
fn one_launch_attempt_at_a_time() {
    let pending = |attempt| Pending {
        attempt,
        index: 0,
        action: None,
        keep_open: true,
        launching: false,
        popup: None,
    };
    let mut state = state(Picker::new("https://x/".into(), Vec::new()), None);
    assert!(state.begin(pending(1)));
    assert!(
        !state.begin(pending(2)),
        "a second selection while one is pending"
    );
    assert!(state.awaiting_token(1), "the first attempt is not dropped");
    assert!(!state.awaiting_token(2));
    assert!(
        state.start(2).is_none(),
        "a stale token or timeout starts nothing"
    );
    assert_eq!(state.start(1).map(|p| p.attempt), Some(1));
    assert!(
        !state.awaiting_token(1),
        "a timeout after the token finds nothing"
    );
    assert_eq!(state.finish(1).map(|p| p.attempt), Some(1));
    assert!(
        state.begin(pending(3)),
        "the next selection starts once it completed"
    );
    assert!(state.awaiting_token(3));
}

#[test]
fn an_attempt_stays_pending_until_its_launch_completes() {
    let pending = |attempt| Pending {
        attempt,
        index: 0,
        action: None,
        keep_open: true,
        launching: false,
        popup: None,
    };
    let mut state = state(Picker::new("https://x/".into(), Vec::new()), None);
    assert!(state.begin(pending(1)));
    assert!(state.start(1).is_some(), "the token arrived: launch 1 runs");
    assert!(
        state.start(1).is_none(),
        "a timeout after the token launches nothing"
    );
    assert!(
        !state.begin(pending(2)),
        "a second selection while the D-Bus Open of 1 is in flight"
    );
    assert!(state.finish(2).is_none(), "a stale result settles nothing");
    assert_eq!(state.finish(1).map(|p| p.attempt), Some(1));
    assert!(
        state.begin(pending(3)),
        "the next selection once 1 completed"
    );
    assert!(
        state.finish(1).is_none(),
        "a reordered or duplicate result of 1 leaves attempt 3 alone"
    );
    assert!(state.awaiting_token(3));
}

#[test]
fn only_an_admitted_keep_open_launch_keeps_the_picker_open() {
    let tile = Tile {
        app_id: "a".into(),
        app_name: "A".into(),
        label: "A".into(),
        icon: None,
        profile: None,
        flatpak: false,
        actions: vec![],
    };
    let selected = |picker: &mut Picker, attempt| {
        let Outcome::Launch {
            index,
            action,
            keep_open,
        } = picker.handle(Input::Activate { ctrl: true })
        else {
            panic!("Ctrl+Enter must launch")
        };
        assert!(keep_open);
        Pending {
            attempt,
            index,
            action,
            keep_open,
            launching: false,
            popup: None,
        }
    };
    let in_flight = Pending {
        attempt: 1,
        index: 0,
        action: None,
        keep_open: false,
        launching: true,
        popup: None,
    };
    let mut busy = state(
        Picker::new("https://x/".into(), vec![tile.clone()]),
        Some(in_flight),
    );
    let rejected = selected(&mut busy.picker, 2);
    assert!(!busy.admit(rejected));
    assert_eq!(
        busy.picker.handle(Input::FocusLost),
        Outcome::Cancel,
        "a rejected Ctrl-selection must not make the picker keep-open"
    );

    let mut idle = state(Picker::new("https://x/".into(), vec![tile]), None);
    let admitted = selected(&mut idle.picker, 3);
    assert!(idle.admit(admitted));
    assert_eq!(
        idle.picker.handle(Input::FocusLost),
        Outcome::None,
        "the target may take focus before its launch result is known"
    );
}

fn tile(app: &str) -> Tile {
    Tile {
        app_id: app.into(),
        app_name: app.into(),
        label: app.into(),
        icon: None,
        profile: None,
        flatpak: false,
        actions: vec![],
    }
}

fn state(picker: Picker, pending: Option<Pending>) -> PickerState {
    PickerState {
        pending,
        ..PickerState::new(picker, Vec::new())
    }
}

fn pending(attempt: u64, keep_open: bool) -> Pending {
    Pending {
        attempt,
        index: 0,
        action: None,
        keep_open,
        launching: false,
        popup: None,
    }
}

#[test]
fn a_new_attempt_clears_the_failure_and_a_rejected_one_leaves_it() {
    let tile = tile("a");
    let mut picker = Picker::new("https://x/".into(), vec![tile.clone()]);
    picker.fail("exited".into(), Some(Target::from(&tile)));
    picker.handle(Input::ToggleDetails);
    let mut busy = state(
        picker.clone(),
        Some(Pending {
            launching: true,
            popup: None,
            ..pending(1, false)
        }),
    );
    assert!(!busy.admit(pending(2, false)));
    assert!(busy.picker.error.is_some() && busy.picker.failed.is_some());
    let mut idle = state(picker, None);
    assert!(idle.admit(pending(2, false)));
    assert_eq!(
        (
            idle.picker.error,
            idle.picker.failed,
            idle.picker.details_open
        ),
        (None, None, false)
    );
}

#[test]
fn a_pinned_digit_launch_keeps_the_picker_and_relaxes_the_focus_once() {
    let mut state = state(Picker::new("https://x/".into(), vec![tile("a")]), None);
    state.picker.handle(Input::TogglePin);
    let mut launch = |attempt| {
        state.picker.handle(Input::DigitPress(1));
        let Outcome::Launch { keep_open, .. } = state.picker.handle(Input::DigitRelease(1)) else {
            panic!("the digit release must launch")
        };
        assert!(keep_open, "the pin makes the digit launch keep-open");
        assert!(state.admit(pending(attempt, keep_open)));
        state.start(attempt);
        let done = state.finish(attempt).expect("the launch completed");
        state.picker.settle(done.index, done.keep_open, None)
    };
    assert_eq!(launch(1), Settled::KeepOpen { relax_focus: true });
    assert_eq!(launch(2), Settled::KeepOpen { relax_focus: false });
    assert!(state.picker.keep_open, "admitted: keep-open until unpinned");
    state.picker.handle(Input::TogglePin);
    assert_eq!(state.picker.handle(Input::FocusLost), Outcome::Cancel);
}

#[test]
fn a_failed_launch_settles_on_its_tile_and_the_next_attempt_dismisses_it() {
    let tiles = vec![tile("a"), tile("b")];
    let mut state = state(Picker::new("https://x/".into(), tiles), None);
    assert!(state.admit(pending(1, false)));
    state.picker.handle(Input::Move(1, 0));
    state.start(1);
    let done = state.finish(1).expect("the launch completed");
    let failure = Some("exited".to_owned());
    assert_eq!(
        state.picker.settle(done.index, done.keep_open, failure),
        Settled::Failed
    );
    assert_eq!(state.picker.focus, 0, "the failed tile has focus again");
    let failed_tile = Target::from(&state.picker.tiles[0]);
    assert_eq!(state.picker.failed, Some(failed_tile));
    assert!(state.admit(pending(2, false)));
    assert_eq!((state.picker.error, state.picker.failed), (None, None));
}

#[test]
fn touch_slop() {
    assert!(!touch_cancels((100.0, 100.0), (105.0, 108.0)));
    assert!(touch_cancels((100.0, 100.0), (100.0, 113.0)));
    assert!(touch_cancels((100.0, 100.0), (91.0, 91.0)));
}

#[test]
fn key_translation() {
    assert_eq!(
        key_from(&keyboard::Key::Named(Named::ArrowLeft)),
        Some(Key::Left)
    );
    assert_eq!(
        key_from(&keyboard::Key::Named(Named::Enter)),
        Some(Key::Enter)
    );
    assert_eq!(
        key_from(&keyboard::Key::Named(Named::ContextMenu)),
        Some(Key::Menu)
    );
    assert_eq!(
        key_from(&keyboard::Key::Character("7".into())),
        Some(Key::Digit(7))
    );
    assert_eq!(
        key_from(&keyboard::Key::Character("c".into())),
        Some(Key::CopyC)
    );
    assert_eq!(key_from(&keyboard::Key::Character("0".into())), None);
    assert_eq!(key_from(&keyboard::Key::Character("x".into())), None);
}
