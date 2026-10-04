mod support;

use std::collections::HashMap;

use tokio::sync::mpsc;
use zbus::zvariant::OwnedValue;

struct FakeApp(mpsc::UnboundedSender<(Vec<String>, Option<String>)>);

#[zbus::interface(name = "org.freedesktop.Application")]
impl FakeApp {
    #[expect(
        clippy::needless_pass_by_value,
        reason = "zbus deserializes method arguments by value"
    )]
    fn open(&self, uris: Vec<String>, platform_data: HashMap<String, OwnedValue>) {
        let token = platform_data
            .get("activation-token")
            .and_then(|v| String::try_from(v.clone()).ok());
        let _ = self.0.send((uris, token));
    }
}

#[tokio::test]
async fn open_via_dbus_reaches_the_apps_object_with_uri_and_token() {
    let bus = support::PrivateBus::start();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let _app = zbus::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .name("org.example.my-app")
        .unwrap()
        .serve_at("/org/example/my_app", FakeApp(tx))
        .unwrap()
        .build()
        .await
        .unwrap();
    let client = bus.connect().await;
    let uri = "https://ex.org/a b?c=%20#f";
    signpost::launch::dbus::open_via_dbus(&client, "org.example.my-app", uri, Some("tok"))
        .await
        .unwrap();
    assert_eq!(
        rx.recv().await.unwrap(),
        (vec![uri.to_owned()], Some("tok".to_owned()))
    );
}
