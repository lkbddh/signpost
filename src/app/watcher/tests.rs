//! Which changes under the watched directories reload the index, how a burst of them is settled, and how the
//! watches follow folders that come and go.

use notify::EventKind as K;
use notify::event::{
    AccessKind, AccessMode, CreateKind, DataChange, ModifyKind, RemoveKind, RenameMode,
};
use std::time::Duration;

use tokio::sync::mpsc;

use super::*;

fn event(kind: K, paths: &[&str]) -> notify::Event {
    paths.iter().fold(notify::Event::new(kind), |event, path| {
        event.add_path(PathBuf::from(path))
    })
}

fn written(path: &str) -> notify::Event {
    event(K::Modify(ModifyKind::Data(DataChange::Content)), &[path])
}

fn nothing_known() -> Directories {
    Directories::default()
}

#[test]
fn desktop_entries_and_mime_lists_change_the_index() {
    let changing = [
        event(
            K::Create(CreateKind::File),
            &["/usr/share/applications/org.example.App.desktop"],
        ),
        event(
            K::Remove(RemoveKind::File),
            &["/home/u/.local/share/applications/kde4/app.desktop"],
        ),
        written("/usr/share/applications/org.example.App.desktop"),
        written("/home/u/.config/mimeapps.list"),
        written("/home/u/.config/cosmic-mimeapps.list"),
        written("/usr/share/applications/mimeapps.list"),
        event(
            K::Modify(ModifyKind::Name(RenameMode::Both)),
            &[
                "/home/u/.config/mimeapps.list.Xk2d9a",
                "/home/u/.config/mimeapps.list",
            ],
        ),
    ];
    for event in &changing {
        assert!(changes_index(event, &nothing_known()), "{event:?}");
    }
}

#[test]
fn other_files_under_a_watched_directory_do_not() {
    let unrelated = [
        written("/home/u/.config/user-dirs.dirs"),
        written("/home/u/.config/cosmic/com.system76.CosmicComp/v1/xdg_activation_filter"),
        written("/usr/share/applications/mimeinfo.cache"),
        written("/usr/share/applications/org.example.App.desktop.bak"),
        event(
            K::Create(CreateKind::File),
            &["/home/u/.config/.mimeapps.list.swp"],
        ),
        event(
            K::Modify(ModifyKind::Name(RenameMode::From)),
            &["/home/u/.config/settings.json.tmp"],
        ),
        event(K::Create(CreateKind::File), &["/home/u/.bash_history"]),
    ];
    for event in &unrelated {
        assert!(!changes_index(event, &nothing_known()), "{event:?}");
    }
}

#[test]
fn a_directory_coming_or_going_changes_the_index_whatever_its_name() {
    for kind in [K::Create(CreateKind::Folder), K::Remove(RemoveKind::Folder)] {
        assert!(changes_index(
            &event(kind, &["/home/u/.local/share/applications"]),
            &nothing_known()
        ));
        assert!(changes_index(
            &event(kind, &["/home/u/.local"]),
            &nothing_known()
        ));
    }
}

#[test]
fn a_kind_that_never_changes_content_does_not_whatever_the_name() {
    let access = K::Access(AccessKind::Open(AccessMode::Any));
    assert!(!changes_index(
        &event(access, &["/usr/share/applications/org.example.App.desktop"]),
        &nothing_known()
    ));
    assert!(!changes_index(
        &event(K::Other, &["/home/u/.config/mimeapps.list"]),
        &nothing_known()
    ));
}

#[test]
fn an_event_for_an_unrelated_file_schedules_no_reload() {
    let (reloads, mut scheduled) = mpsc::channel(8);
    let known = nothing_known();
    schedule_reload(
        &reloads,
        &Ok(written("/home/u/.config/user-dirs.dirs")),
        &known,
    );
    assert!(scheduled.try_recv().is_err());
    schedule_reload(
        &reloads,
        &Ok(written("/usr/share/applications/org.example.App.desktop")),
        &known,
    );
    assert!(scheduled.try_recv().is_ok());
}

/// The watcher could not say what changed (its queue overflowed, or a watch failed), so the index reads
/// everything again.
#[test]
fn events_the_watcher_lost_schedule_a_reload() {
    let lost = [
        Ok(notify::Event::new(K::Other).set_flag(notify::event::Flag::Rescan)),
        Err(notify::Error::generic("the watch was lost")),
    ];
    for event in lost {
        let (reloads, mut scheduled) = mpsc::channel(8);
        let log = crate::test_support::logs::logged(|| {
            schedule_reload(&reloads, &event, &nothing_known());
        });
        assert!(scheduled.try_recv().is_ok(), "{event:?}");
        assert!(log.contains("reload"), "{log}");
    }
}

/// The index read at startup predates the watches, so a change made in between is caught by a reload once they
/// are registered.
#[tokio::test]
async fn the_index_reloads_once_its_folders_are_watched() {
    use cosmic::iced::futures::StreamExt;
    let tmp = tempfile::tempdir().unwrap();
    let changes = index_changes(vec![(tmp.path().to_owned(), true)]);
    let first = tokio::time::timeout(Duration::from_secs(5), Box::pin(changes).next()).await;
    assert!(
        matches!(first, Ok(Some(Message::IndexChanged))),
        "{first:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn a_burst_of_changes_reloads_once() {
    let (reloads, mut scheduled) = mpsc::channel(8);
    let known = nothing_known();
    let relevant = |n: usize| Ok(written(&format!("/usr/share/applications/app{n}.desktop")));
    for n in 0..20 {
        schedule_reload(&reloads, &relevant(n), &known);
    }
    assert_eq!(next_change(&mut scheduled).await, Some(()));
    assert!(scheduled.try_recv().is_err(), "the burst was one change");

    let (settled, ()) = tokio::join!(next_change(&mut scheduled), async {
        schedule_reload(&reloads, &relevant(0), &known);
        tokio::time::sleep(RELOAD_DEBOUNCE / 2).await;
        schedule_reload(&reloads, &relevant(1), &known);
        schedule_reload(&reloads, &relevant(2), &known);
    });
    assert_eq!(settled, Some(()));
    assert!(
        scheduled.try_recv().is_err(),
        "events inside the window are the same change"
    );

    schedule_reload(&reloads, &relevant(3), &known);
    assert_eq!(
        next_change(&mut scheduled).await,
        Some(()),
        "a later change reloads again"
    );
}

#[tokio::test(start_paused = true)]
async fn no_change_comes_once_nothing_can_schedule_one() {
    let (reloads, mut scheduled) = mpsc::channel(8);
    drop(reloads);
    assert_eq!(next_change(&mut scheduled).await, None);
}

/// How long a change that should have scheduled a reload is given to.
const SETTLE: Duration = Duration::from_secs(5);
/// How long without a reload counts as the end of what a change sets off.
const QUIET: Duration = Duration::from_millis(300);

/// Watches `root` as the app does, with what it registered, the directories it knows and the reloads its
/// events schedule.
fn watching(
    root: &std::path::Path,
) -> (
    notify::RecommendedWatcher,
    Vec<(PathBuf, bool)>,
    Directories,
    mpsc::Receiver<()>,
) {
    let (reloads, scheduled) = mpsc::channel(8);
    let directories = Directories::default();
    let known = directories.clone();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        schedule_reload(&reloads, &event, &known);
    })
    .expect("a watcher");
    let mut registered = Vec::new();
    reconcile_watches(&mut watcher, &mut registered, &[(root.to_owned(), true)]);
    directories.remember(&registered);
    (watcher, registered, directories, scheduled)
}

async fn reloads_after(scheduled: &mut mpsc::Receiver<()>, change: impl FnOnce()) -> bool {
    while tokio::time::timeout(QUIET, scheduled.recv()).await.is_ok() {}
    change();
    tokio::time::timeout(SETTLE, scheduled.recv()).await.is_ok()
}

#[tokio::test]
async fn a_populated_folder_moved_into_or_out_of_the_applications_root_reloads_the_index() {
    for name in ["vendor", "vendor.v1"] {
        let dir = tempfile::tempdir().expect("a scratch directory");
        let root = dir.path().join("applications");
        let elsewhere = dir.path().join(name);
        std::fs::create_dir_all(&root).expect("the root");
        std::fs::create_dir_all(&elsewhere).expect("a folder");
        std::fs::write(
            elsewhere.join("org.example.App.desktop"),
            "[Desktop Entry]\n",
        )
        .expect("an app");
        let (mut watcher, mut registered, directories, mut scheduled) = watching(&root);

        let arrived = reloads_after(&mut scheduled, || {
            std::fs::rename(&elsewhere, root.join(name)).expect("the move in");
        })
        .await;
        assert!(arrived, "{name}: a folder of apps moved in changed nothing");

        // What the app does after every change, so that the folder's own contents are watched.
        reconcile_watches(&mut watcher, &mut registered, &[(root.clone(), true)]);
        directories.remember(&registered);
        let added = reloads_after(&mut scheduled, || {
            std::fs::write(root.join(name).join("other.desktop"), "[Desktop Entry]\n")
                .expect("an app");
        })
        .await;
        assert!(
            added,
            "{name}: an app added to the moved folder went unseen"
        );

        let left = reloads_after(&mut scheduled, || {
            std::fs::rename(root.join(name), &elsewhere).expect("the move out");
        })
        .await;
        assert!(left, "{name}: a folder of apps moved out changed nothing");
    }
}

#[test]
fn a_rename_to_a_directory_changes_the_index_whatever_its_name() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let folder = dir.path().join("applications/vendor.v1");
    std::fs::create_dir_all(&folder).expect("a folder");
    let folder = folder.to_str().expect("a path");
    for mode in [RenameMode::To, RenameMode::Any] {
        let moved = event(K::Modify(ModifyKind::Name(mode)), &[folder]);
        assert!(changes_index(&moved, &nothing_known()), "{moved:?}");
    }
    let moved_within = event(
        K::Modify(ModifyKind::Name(RenameMode::Both)),
        &["/home/u/.local/share/applications/old.v0", folder],
    );
    assert!(
        changes_index(&moved_within, &nothing_known()),
        "{moved_within:?}"
    );
}

#[test]
fn a_rename_from_a_directory_the_watcher_knew_changes_the_index_whatever_its_name() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let root = dir.path().join("applications");
    let folder = root.join("vendor.v1");
    let nested = folder.join("kde.d");
    std::fs::create_dir_all(&nested).expect("a folder of folders");
    let directories = Directories::default();
    directories.remember(&[(root.clone(), true)]);
    std::fs::remove_dir_all(&folder).expect("the folder has gone");

    for gone in [&folder, &nested, &root] {
        let moved = event(
            K::Modify(ModifyKind::Name(RenameMode::From)),
            &[gone.to_str().expect("a path")],
        );
        assert!(changes_index(&moved, &directories), "{moved:?}");
    }
    let unknown = event(
        K::Modify(ModifyKind::Name(RenameMode::From)),
        &[root.join("other.v1").to_str().expect("a path")],
    );
    assert!(
        !changes_index(&unknown, &directories),
        "a path that was never a directory: {unknown:?}"
    );
}

#[test]
fn a_directory_is_known_until_the_watches_are_registered_again() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let root = dir.path().join("applications");
    std::fs::create_dir_all(root.join("vendor.v1")).expect("a folder");
    let directories = Directories::default();
    directories.remember(&[(root.clone(), true)]);
    std::fs::remove_dir(root.join("vendor.v1")).expect("the folder has gone");
    assert!(directories.includes(&root.join("vendor.v1")));

    directories.remember(&[(root.clone(), true)]);

    assert!(!directories.includes(&root.join("vendor.v1")));
    assert!(directories.includes(&root), "the root is watched");
}

#[test]
fn a_renamed_file_is_not_a_folder_whatever_its_name() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let root = dir.path().join("applications");
    std::fs::create_dir_all(&root).expect("the root");
    let directories = Directories::default();
    directories.remember(&[(root.clone(), true)]);
    for name in ["README", "notes.v1", "cache.json"] {
        let file = root.join(name);
        std::fs::write(&file, "x").expect("a file");
        let renamed = event(
            K::Modify(ModifyKind::Name(RenameMode::To)),
            &[file.to_str().expect("a path")],
        );
        assert!(!changes_index(&renamed, &directories), "{renamed:?}");
    }
}

#[test]
fn a_renamed_file_that_is_not_an_app_list_changes_nothing() {
    let renamed = event(
        K::Modify(ModifyKind::Name(RenameMode::Both)),
        &["/home/u/.config/a.tmp", "/home/u/.config/b.json"],
    );
    assert!(!changes_index(&renamed, &nothing_known()), "{renamed:?}");
}

#[test]
fn watch_targets_fall_back_to_existing_ancestors() {
    use std::path::{Path, PathBuf};
    let existing = [
        "/home/u",
        "/home/u/.local",
        "/usr/share/applications",
        "/etc/xdg",
    ];
    let exists = |p: &Path| existing.iter().any(|e| Path::new(e) == p);
    let wanted = vec![
        (PathBuf::from("/usr/share/applications"), true),
        (PathBuf::from("/home/u/.local/share/applications"), true),
        (PathBuf::from("/home/u/.config"), false),
        (PathBuf::from("/etc/xdg"), false),
    ];
    assert_eq!(
        watch_targets(&wanted, &exists),
        vec![
            (PathBuf::from("/usr/share/applications"), true),
            (PathBuf::from("/home/u/.local"), false),
            (PathBuf::from("/home/u"), false),
            (PathBuf::from("/etc/xdg"), false),
        ]
    );
}

#[test]
fn recreated_directory_is_watched_again_after_reconcile() {
    use std::time::Duration;
    let tmp = tempfile::tempdir().unwrap();
    let apps = tmp.path().join("applications");
    std::fs::create_dir(&apps).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res {
            let _ = tx.send(ev.paths);
        }
    })
    .unwrap();
    let wanted = vec![(apps.clone(), true)];
    let mut registered = Vec::new();
    reconcile_watches(&mut watcher, &mut registered, &wanted);
    std::fs::remove_dir_all(&apps).unwrap();
    std::fs::create_dir(&apps).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    while rx.try_recv().is_ok() {}
    reconcile_watches(&mut watcher, &mut registered, &wanted);
    let file = apps.join("new.desktop");
    std::fs::write(&file, "[Desktop Entry]\n").unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let mut seen = false;
    while std::time::Instant::now() < deadline && !seen {
        if let Ok(paths) = rx.recv_timeout(Duration::from_millis(200)) {
            seen = paths.iter().any(|p| p == &file);
        }
    }
    assert!(
        seen,
        "a change inside the recreated directory must be observed"
    );
}

#[test]
fn only_content_changes_are_relevant() {
    use notify::EventKind as K;
    use notify::event::{
        AccessKind, CreateKind, DataChange, MetadataKind, ModifyKind, RemoveKind, RenameMode,
    };
    assert!(is_relevant(K::Create(CreateKind::File)));
    assert!(is_relevant(K::Remove(RemoveKind::Folder)));
    assert!(is_relevant(K::Modify(ModifyKind::Data(
        DataChange::Content
    ))));
    assert!(is_relevant(K::Modify(ModifyKind::Name(RenameMode::To))));
    assert!(!is_relevant(K::Access(AccessKind::Open(
        notify::event::AccessMode::Any
    ))));
    assert!(!is_relevant(K::Modify(ModifyKind::Metadata(
        MetadataKind::AccessTime
    ))));
    assert!(!is_relevant(K::Other));
}

#[test]
fn reconciliation_settles_without_relevant_events() {
    use std::time::Duration;
    let tmp = tempfile::tempdir().unwrap();
    let apps = tmp.path().join("applications");
    std::fs::create_dir_all(apps.join("nested/deeper")).unwrap();
    std::fs::write(apps.join("nested/x.desktop"), "[Desktop Entry]\n").unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res {
            let _ = tx.send(ev.kind);
        }
    })
    .unwrap();
    let wanted = vec![(apps, true)];
    let mut registered = Vec::new();
    for _ in 0..3 {
        reconcile_watches(&mut watcher, &mut registered, &wanted);
    }
    std::thread::sleep(Duration::from_millis(500));
    let relevant: Vec<_> = rx.try_iter().filter(|kind| is_relevant(*kind)).collect();
    assert!(
        relevant.is_empty(),
        "registration alone must not look like a change: {relevant:?}"
    );
}
