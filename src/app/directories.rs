//! The directories under the watched roots. A directory that has been moved away cannot be asked what
//! it was, so the ones the watcher registered are remembered.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

/// Read by the watcher's callback on its own thread, and replaced by the task that registers the watches.
#[derive(Clone, Default)]
pub struct Directories(Arc<Mutex<HashSet<PathBuf>>>);

impl Directories {
    /// Replaces what is known with the registered roots and every directory under the recursive ones.
    /// A symlink is not followed.
    pub fn remember(&self, registered: &[(PathBuf, bool)]) {
        let mut known = HashSet::new();
        for (root, recursive) in registered {
            known.insert(root.clone());
            if *recursive {
                collect_under(root, &mut known);
            }
        }
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = known;
    }

    /// Whether `path` is a directory now, or was one when the watches were last registered.
    pub fn includes(&self, path: &Path) -> bool {
        path.is_dir()
            || self
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .contains(path)
    }
}

fn collect_under(dir: &Path, known: &mut HashSet<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            tracing::debug!(dir = %dir.display(), %error, "cannot list a watched directory");
            return;
        }
    };
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            known.insert(entry.path());
            collect_under(&entry.path(), known);
        }
    }
}
