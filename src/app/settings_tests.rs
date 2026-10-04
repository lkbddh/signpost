//! The settings window as the app opens and raises it.
//!
//! Building the app reads libcosmic's configuration and the app index, so these tests run again in a child
//! process whose home is a scratch directory.

use super::world::World;
use super::*;
use crate::test_support::headless;

const SCRATCH_HOME: &str = "SIGNPOST_APP_SETTINGS_SCRATCH_HOME";

/// Runs test `name` again with its home in a scratch directory, and says whether this is that run.
fn in_scratch_home(name: &str) -> bool {
    headless::in_scratch_home(SCRATCH_HOME, &format!("app::settings_tests::{name}"))
}

fn record_error_shown(app: &App) -> bool {
    app.settings
        .as_ref()
        .expect("settings")
        .model()
        .record_error
        .is_some()
}

/// Nothing watches the record of the saved defaults, so raising the window is when a repair shows.
#[test]
fn raising_the_settings_window_reads_the_saved_defaults_again() {
    if !in_scratch_home("raising_the_settings_window_reads_the_saved_defaults_again") {
        return;
    }
    let mut world = World::new();
    let _runtime = world.runtime.enter();
    drop(world.app.open_settings(None));
    let id = world.app.settings.as_ref().expect("settings").id();
    drop(world.app.settings_opened(id));
    assert!(!record_error_shown(&world.app));

    let record = world.app.paths.state_home.join("signpost/restore.json");
    std::fs::create_dir_all(record.parent().unwrap()).unwrap();
    std::fs::write(&record, "{ not json").unwrap();
    drop(world.app.open_settings(None));
    assert!(record_error_shown(&world.app), "the broken record shows");

    std::fs::remove_file(&record).unwrap();
    drop(world.app.open_settings(Some("a-token".into())));
    assert!(
        !record_error_shown(&world.app),
        "the removed record is gone"
    );
}
