//! Gives a launched app its own systemd scope, so it no longer lives in Signpost's service cgroup.

use std::time::Duration;

use futures_util::StreamExt;
use zbus::zvariant::{OwnedObjectPath, Value};

use crate::index::AppEntry;

const SYSTEMD: &str = "org.freedesktop.systemd1";
const SYSTEMD_PATH: &str = "/org/freedesktop/systemd1";
const MANAGER: &str = "org.freedesktop.systemd1.Manager";
/// Fails if a unit of that name exists, rather than joining it.
const START_MODE: &str = "fail";
/// The scope is removed once its processes are gone, failed or not.
const COLLECT_MODE: &str = "inactive-or-failed";
/// What a job that did what it was for ends as.
const JOB_DONE: &str = "done";
/// The app waits for its scope, so a manager that has not run the job in this time is given up on.
pub const SCOPE_TIMEOUT: Duration = Duration::from_secs(2);

/// The name the XDG desktop environment spec gives the scope of an app a launcher started, its pid the suffix.
fn scope_name(entry: &AppEntry, pid: u32) -> String {
    unit_name(&entry.id, &pid.to_string())
}

/// A scope's unit name as the XDG desktop entry spec names an app's scope: `app-signpost-`, the desktop id escaped
/// as systemd escapes a unit name's part, `-`, then `suffix`. An id too long for a unit name, 255 bytes, is replaced
/// by 16 hex digits of a hash of it.
#[must_use]
pub fn unit_name(desktop_id: &str, suffix: &str) -> String {
    let unit = format!("app-signpost-{}-{suffix}.scope", escaped(desktop_id));
    if unit.len() <= MAX_UNIT_NAME {
        return unit;
    }
    let mut hasher = std::hash::DefaultHasher::new();
    std::hash::Hash::hash(desktop_id, &mut hasher);
    format!(
        "app-signpost-{:016x}-{suffix}.scope",
        std::hash::Hasher::finish(&hasher)
    )
}

/// The longest unit name systemd takes, in bytes.
const MAX_UNIT_NAME: usize = 255;

/// `part` as systemd escapes a part of a unit name: ASCII letters, digits, `:`, `_` and `.` stay, except a leading
/// `.`; every other byte becomes `\xNN`. A `-` too, as the name uses it between its parts.
fn escaped(part: &str) -> String {
    part.bytes()
        .enumerate()
        .map(|(at, byte)| {
            let kept = byte.is_ascii_alphanumeric()
                || matches!(byte, b':' | b'_')
                || (byte == b'.' && at > 0);
            if kept {
                char::from(byte).to_string()
            } else {
                format!("\\x{byte:02x}")
            }
        })
        .collect()
}

/// Starts the scope and waits for its job to be removed, which gives how the job ended.
///
/// The call replies once the job is queued, and the pid only moves when the job runs. The signal is
/// listened for before the call, as a job that runs at once can send it ahead of the reply.
async fn start_scope(conn: &zbus::Connection, entry: &AppEntry, pid: u32) -> zbus::Result<String> {
    let manager = zbus::Proxy::new(conn, SYSTEMD, SYSTEMD_PATH, MANAGER).await?;
    let mut removals = manager.receive_signal("JobRemoved").await?;
    let description = format!("{} launched by Signpost", entry.name);
    let properties: Vec<(&str, Value<'_>)> = vec![
        ("Description", description.as_str().into()),
        ("PIDs", vec![pid].into()),
        ("CollectMode", COLLECT_MODE.into()),
    ];
    let aux: Vec<(&str, Vec<(&str, Value<'_>)>)> = Vec::new();
    let job: OwnedObjectPath = manager
        .call(
            "StartTransientUnit",
            &(scope_name(entry, pid), START_MODE, properties, aux),
        )
        .await?;
    while let Some(signal) = removals.next().await {
        let (_id, removed, _unit, result): (u32, OwnedObjectPath, String, String) =
            signal.body().deserialize()?;
        if removed == job {
            return Ok(result);
        }
    }
    Err(zbus::Error::Failure(
        "the manager's signals ended before the job was removed".to_owned(),
    ))
}

/// Moves `pid` into a transient scope of its own under the user's systemd manager, and returns once the
/// move has been made or given up on. Best effort: with no manager, or one that refuses, the app stays where it
/// is; a job the manager has not run within `SCOPE_TIMEOUT` stays queued and may still move it later. Either way
/// the launch is none the worse.
pub async fn enter_own_scope(conn: &zbus::Connection, entry: &AppEntry, pid: u32) {
    match tokio::time::timeout(SCOPE_TIMEOUT, start_scope(conn, entry, pid)).await {
        Ok(Ok(result)) if result == JOB_DONE => {}
        Ok(Ok(result)) => {
            tracing::debug!(%result, pid, app = %entry.id, "the app stays in Signpost's own cgroup");
        }
        Ok(Err(error)) => {
            tracing::debug!(%error, pid, app = %entry.id, "the app stays in Signpost's own cgroup");
        }
        Err(_) => {
            tracing::debug!(pid, app = %entry.id, "the manager has not run the job yet: the app may move to its scope later");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tokio::sync::mpsc;
    use zbus::zvariant::Value;

    use super::*;
    use crate::launch::tests::entry;
    use crate::test_support::peer::{self, AnsweringApp, FakeManager, SYSTEMD_PATH};

    const PID: u32 = 4242;
    /// How long an attempt that must still be waiting is watched.
    const HELD: Duration = Duration::from_millis(100);
    /// How long an attempt that must end is waited for before the test calls it hung; only a guard, as the
    /// deadline itself is checked by the time it took.
    const HANG: Duration = Duration::from_secs(30);
    const OTHER_JOB: &str = "/org/freedesktop/systemd1/job/2";

    fn example() -> AppEntry {
        entry("org.example.App", "Example", &["example", "%u"], &[])
    }

    #[tokio::test]
    async fn the_app_moves_into_a_scope_named_for_it_and_its_pid() {
        let (asked, mut units) = mpsc::unbounded_channel();
        let (conn, _manager) =
            peer::connect(|manager| manager.serve_at(SYSTEMD_PATH, FakeManager::starting(asked)))
                .await;
        let started = tokio::time::Instant::now();

        enter_own_scope(&conn, &example(), PID).await;

        assert!(
            started.elapsed() < SCOPE_TIMEOUT,
            "a job removed before the reply was missed"
        );

        let unit = units.try_recv().expect("the manager was asked");
        assert_eq!(unit.name, "app-signpost-org.example.App-4242.scope");
        assert_eq!(unit.mode, "fail");
        let properties: HashMap<&str, &Value<'_>> = unit
            .properties
            .iter()
            .map(|(name, value)| (name.as_str(), &**value))
            .collect();
        assert_eq!(
            properties,
            HashMap::from([
                ("Description", &Value::from("Example launched by Signpost")),
                ("PIDs", &Value::from(vec![PID])),
                ("CollectMode", &Value::from("inactive-or-failed")),
            ])
        );
        assert!(units.try_recv().is_err(), "one call");
    }

    #[tokio::test]
    async fn a_manager_that_refuses_leaves_the_app_where_it_is() {
        let (asked, mut units) = mpsc::unbounded_channel();
        let (conn, _manager) = peer::connect(|manager| {
            manager.serve_at(SYSTEMD_PATH, FakeManager::refusing(asked, "no such luck"))
        })
        .await;

        enter_own_scope(&conn, &example(), PID).await;

        assert!(units.try_recv().is_ok(), "the manager was asked");
    }

    #[tokio::test]
    async fn a_manager_that_never_answers_is_given_up_on_at_the_deadline() {
        let (asked, mut units) = mpsc::unbounded_channel();
        let (conn, _manager) =
            peer::connect(|manager| manager.serve_at(SYSTEMD_PATH, FakeManager::silent(asked)))
                .await;
        let started = tokio::time::Instant::now();

        // The outer bound only trips if the attempt never gives up on its own.
        tokio::time::timeout(HANG, enter_own_scope(&conn, &example(), PID))
            .await
            .expect("the attempt outlived its deadline");

        assert!(units.try_recv().is_ok(), "the manager was asked");
        assert!(started.elapsed() >= SCOPE_TIMEOUT, "gave up early");
    }

    /// An attempt against a manager that has replied with a queued job and has not run it, and the
    /// manager's own connection, which the test sends its signals on.
    async fn attempt_on_a_queued_job() -> (tokio::task::JoinHandle<()>, zbus::Connection) {
        let (asked, mut units) = mpsc::unbounded_channel();
        let (conn, manager) =
            peer::connect(|manager| manager.serve_at(SYSTEMD_PATH, FakeManager::queuing(asked)))
                .await;
        let attempt = tokio::spawn(async move { enter_own_scope(&conn, &example(), PID).await });
        units.recv().await.expect("the manager was asked");
        (attempt, manager)
    }

    #[tokio::test]
    async fn the_attempt_lasts_until_the_job_has_run_not_until_the_manager_replies() {
        let (attempt, manager) = attempt_on_a_queued_job().await;

        tokio::time::sleep(HELD).await;
        assert!(!attempt.is_finished(), "ended with the job still queued");

        peer::remove_job(&manager, peer::JOB, "done").await;
        tokio::time::timeout(SCOPE_TIMEOUT, attempt)
            .await
            .expect("the job's removal ended the attempt")
            .expect("the task ran");
    }

    #[tokio::test]
    async fn another_job_being_removed_does_not_end_it_but_its_own_does_however_it_ended() {
        let (attempt, manager) = attempt_on_a_queued_job().await;

        peer::remove_job(&manager, OTHER_JOB, "done").await;
        tokio::time::sleep(HELD).await;
        assert!(!attempt.is_finished(), "ended by another job's removal");

        peer::remove_job(&manager, peer::JOB, "failed").await;
        tokio::time::timeout(SCOPE_TIMEOUT, attempt)
            .await
            .expect("the job's removal ended the attempt")
            .expect("the task ran");
    }

    #[tokio::test]
    async fn a_job_that_is_never_removed_ends_the_attempt_at_the_deadline() {
        let started = tokio::time::Instant::now();
        let (attempt, _manager) = attempt_on_a_queued_job().await;

        tokio::time::timeout(HANG, attempt)
            .await
            .expect("the attempt outlived its deadline")
            .expect("the task ran");

        assert!(started.elapsed() >= SCOPE_TIMEOUT, "gave up early");
    }

    #[tokio::test]
    async fn no_manager_at_all_leaves_the_app_where_it_is() {
        // The bus answers a call to a name nobody owns with an error; this peer answers one to a path
        // nothing is exported at the same way.
        let (conn, _peer) =
            peer::connect(|other| other.serve_at("/org/example/Other", AnsweringApp)).await;
        enter_own_scope(&conn, &example(), PID).await;
    }

    #[test]
    fn a_scope_is_named_as_systemd_takes_a_unit_name_whatever_the_desktop_id() {
        let mut odd = example();
        odd.id = "my app-x".into();
        assert_eq!(
            scope_name(&odd, 42),
            r"app-signpost-my\x20app\x2dx-42.scope"
        );
        odd.id = "x".repeat(300);
        let long = scope_name(&odd, 42);
        assert!(long.len() <= 255, "{} bytes", long.len());
        assert!(
            long.starts_with("app-signpost-") && long.ends_with("-42.scope"),
            "{long}"
        );
    }
}
