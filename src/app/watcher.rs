//! Which changes to the files the index reads are worth a reload, and the stream that watches for them.

use std::path::PathBuf;
use std::time::Duration;

use cosmic::iced::futures::Stream;

use super::Message;
use super::directories::Directories;

/// How long after a change to the index's files the index reloads, so the changes made with it reload once.
const RELOAD_DEBOUNCE: Duration = Duration::from_millis(500);

/// What to actually watch: each wanted dir if it exists (with its mode), otherwise its nearest existing
/// ancestor non-recursively, so its later creation is noticed and registration can be redone.
fn watch_targets(
    wanted: &[(PathBuf, bool)],
    exists: &dyn Fn(&std::path::Path) -> bool,
) -> Vec<(PathBuf, bool)> {
    let mut out: Vec<(PathBuf, bool)> = Vec::new();
    for (dir, recursive) in wanted {
        let target = if exists(dir) {
            (dir.clone(), *recursive)
        } else {
            match dir.ancestors().skip(1).find(|a| exists(a)) {
                Some(a) => (a.to_path_buf(), false),
                None => continue,
            }
        };
        if !out.contains(&target) {
            out.push(target);
        }
    }
    out
}

/// Only content-affecting changes refresh the index. Access/Open events (which notify's own recursive
/// registration produces) and metadata-only changes are ignored, so reconciliation cannot feed itself.
fn is_relevant(kind: notify::EventKind) -> bool {
    use notify::EventKind as K;
    use notify::event::ModifyKind as M;
    match kind {
        K::Create(_) | K::Remove(_) => true,
        K::Modify(m) => matches!(m, M::Data(_) | M::Name(_) | M::Any),
        _ => false,
    }
}

/// Whether `event` changes what the index reads: a desktop entry, a `mimeapps.list`, or a directory
/// coming, going or moving, which changes what is watched and brings its apps along. Anything else
/// written under a watched directory is none of the index's business.
fn changes_index(event: &notify::Event, directories: &Directories) -> bool {
    use notify::EventKind as K;
    use notify::event::{CreateKind, RemoveKind};
    is_relevant(event.kind)
        && (matches!(
            event.kind,
            K::Create(CreateKind::Folder) | K::Remove(RemoveKind::Folder)
        ) || moves_folder(event, directories)
            || event.paths.iter().any(|path| is_index_file(path)))
}

/// A rename of a directory, which brings its apps or takes them away without an event for any of them.
/// A name does not say what it names: the directory is asked for if it is there, and remembered if not.
fn moves_folder(event: &notify::Event, directories: &Directories) -> bool {
    matches!(
        event.kind,
        notify::EventKind::Modify(notify::event::ModifyKind::Name(_))
    ) && event.paths.iter().any(|path| directories.includes(path))
}

fn is_index_file(path: &std::path::Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension == "desktop")
        || path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().ends_with("mimeapps.list"))
}

/// Queues a reload for an event that changes what the index reads, and for one the watcher lost: an overflowed
/// queue or a failed watch says nothing of what changed, so everything is read again.
fn schedule_reload(
    reloads: &tokio::sync::mpsc::Sender<()>,
    event: &notify::Result<notify::Event>,
    directories: &Directories,
) {
    let reload = match event {
        Ok(event) if event.need_rescan() => {
            tracing::warn!("the watcher lost events; the index will reload");
            true
        }
        Ok(event) => changes_index(event, directories),
        Err(e) => {
            tracing::warn!(error = %e, "the watcher failed; the index will reload");
            true
        }
    };
    if reload {
        let _ = reloads.try_send(());
    }
}

/// Waits for a change to what the index reads, then gathers what else changes within [`RELOAD_DEBOUNCE`] of it:
/// one reload for the whole window, however many events came in it. `None` once nothing will schedule a reload
/// any more.
async fn next_change(reloads: &mut tokio::sync::mpsc::Receiver<()>) -> Option<()> {
    reloads.recv().await?;
    tokio::time::sleep(RELOAD_DEBOUNCE).await;
    while reloads.try_recv().is_ok() {}
    Some(())
}

/// Drop every registration and register the current `watch_targets` again. notify silently loses the
/// watch of a deleted directory; re-registering from scratch makes recreated roots watched again.
fn reconcile_watches(
    watcher: &mut impl notify::Watcher,
    registered: &mut Vec<(PathBuf, bool)>,
    wanted: &[(PathBuf, bool)],
) {
    for (dir, _) in registered.drain(..) {
        let _ = watcher.unwatch(&dir);
    }
    for (dir, recursive) in watch_targets(wanted, &|p: &std::path::Path| p.is_dir()) {
        let mode = if recursive {
            notify::RecursiveMode::Recursive
        } else {
            notify::RecursiveMode::NonRecursive
        };
        match watcher.watch(&dir, mode) {
            Ok(()) => registered.push((dir, recursive)),
            Err(e) => {
                tracing::warn!(dir = %dir.display(), error = %e, "cannot watch for app/association changes");
            }
        }
    }
}

/// Watches `wanted` (each folder with whether its subfolders are watched too) and sends
/// [`Message::IndexChanged`] after each change to what the index reads.
pub(super) fn index_changes(wanted: Vec<(PathBuf, bool)>) -> impl Stream<Item = Message> {
    cosmic::iced::stream::channel(
        1,
        |mut out: cosmic::iced::futures::channel::mpsc::Sender<Message>| async move {
            use cosmic::iced::futures::SinkExt;
            let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(8);
            let directories = Directories::default();
            let known = directories.clone();
            let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                schedule_reload(&tx, &res, &known);
            });
            let Ok(mut watcher) = watcher else {
                tracing::warn!("desktop/mimeapps watcher unavailable; the index will not refresh");
                return std::future::pending::<()>().await;
            };
            // Watching directories also catches creation and atomic replacement.
            let mut registered: Vec<(PathBuf, bool)> = Vec::new();
            reconcile_watches(&mut watcher, &mut registered, &wanted);
            directories.remember(&registered);
            // The index read at startup may predate these watches: one reload covers what changed in between.
            if out.send(Message::IndexChanged).await.is_err() {
                return;
            }
            while next_change(&mut rx).await.is_some() {
                // Directories may have been created, deleted or recreated: rebuild every watch.
                reconcile_watches(&mut watcher, &mut registered, &wanted);
                directories.remember(&registered);
                if out.send(Message::IndexChanged).await.is_err() {
                    return;
                }
            }
            std::future::pending::<()>().await;
        },
    )
}

#[cfg(test)]
mod tests;
