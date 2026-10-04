use std::collections::HashMap;
use std::time::Duration;

use zbus::zvariant::Value;

/// How long a launch waits for the app to answer `Open` before giving up on it.
pub const DBUS_LAUNCH_TIMEOUT: Duration = Duration::from_secs(10);

/// Object path for a `DBusActivatable` desktop id (Desktop Entry spec, "D-Bus Activation"):
/// `org.example.App` → `/org/example/App`, with `-` mapped to `_`.
#[must_use]
pub fn object_path_for(desktop_id: &str) -> String {
    format!("/{}", desktop_id.replace('.', "/").replace('-', "_"))
}

/// Launch through `org.freedesktop.Application.Open` on the app's own bus name.
///
/// # Errors
/// The proxy cannot be created, or the `Open` call fails (name not owned or not activatable, the
/// app returns an error), or the app does not answer within `DBUS_LAUNCH_TIMEOUT`.
pub async fn open_via_dbus(
    conn: &zbus::Connection,
    desktop_id: &str,
    uri: &str,
    token: Option<&str>,
) -> zbus::Result<()> {
    let path = object_path_for(desktop_id);
    let proxy = zbus::Proxy::new(
        conn,
        desktop_id,
        path.as_str(),
        "org.freedesktop.Application",
    )
    .await?;
    let mut platform_data: HashMap<&str, Value<'_>> = HashMap::new();
    if let Some(token) = token {
        platform_data.insert("activation-token", Value::from(token));
        platform_data.insert("desktop-startup-id", Value::from(token));
    }
    tokio::time::timeout(
        DBUS_LAUNCH_TIMEOUT,
        proxy.call_method("Open", &(vec![uri], platform_data)),
    )
    .await
    .map_err(|_| zbus::Error::Failure("the app did not answer".to_owned()))??;
    Ok(())
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc;

    use super::*;
    use crate::test_support::peer::{self, SilentApp};

    #[test]
    fn object_paths() {
        assert_eq!(
            super::object_path_for("org.gnome.Nautilus"),
            "/org/gnome/Nautilus"
        );
        assert_eq!(
            super::object_path_for("org.example.my-app"),
            "/org/example/my_app"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn an_app_that_never_answers_ends_the_launch_at_the_deadline() {
        let (entered, mut reached) = mpsc::unbounded_channel();
        let (client, _peer) =
            peer::connect(|app| app.serve_at("/org/example/App", SilentApp(entered))).await;
        let started = tokio::time::Instant::now();
        // Virtual time: the outer bound only trips if the launch never gives up on its own.
        let outcome = tokio::time::timeout(
            DBUS_LAUNCH_TIMEOUT * 2,
            open_via_dbus(&client, "org.example.App", "https://x/", None),
        )
        .await
        .expect("the launch ended by itself");
        let error = outcome.expect_err("an app that does not answer fails the launch");
        assert_eq!(error.to_string(), "the app did not answer");
        assert!(started.elapsed() >= DBUS_LAUNCH_TIMEOUT, "gave up early");
        assert!(reached.try_recv().is_ok(), "the call reached the app");
    }
}
