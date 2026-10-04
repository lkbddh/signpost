mod support;

use std::time::Duration;

use signpost::bus::{BusEvent, Daemon, ElectError, Outcome, Role, elect, forward};
use signpost::picker::MAX_WAITING_LINKS;

const TEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Bounds a test body so a missing reply fails the test instead of hanging it.
async fn bounded<T>(name: &str, body: impl Future<Output = T>) -> T {
    tokio::time::timeout(TEST_TIMEOUT, body)
        .await
        .unwrap_or_else(|_| panic!("{name} timed out"))
}

fn builder(bus: &support::PrivateBus) -> zbus::connection::Builder<'static> {
    zbus::connection::Builder::address(bus.address.clone().as_str()).unwrap()
}

async fn has_owner(bus: &support::PrivateBus) -> bool {
    let c = bus.connect().await;
    zbus::fdo::DBusProxy::new(&c)
        .await
        .unwrap()
        .name_has_owner(signpost::bus::APP_ID.try_into().unwrap())
        .await
        .unwrap()
}

async fn won(bus: &support::PrivateBus, role: Role) -> Daemon {
    match elect(builder(bus), role).await.unwrap() {
        Outcome::Won(d) => d,
        _ => panic!("must win"),
    }
}

#[tokio::test]
async fn winner_admits_open_only_after_ready() {
    let bus = support::PrivateBus::start();
    bounded("winner_admits_open_only_after_ready", async {
        let Daemon {
            conn: _conn,
            readiness,
            mut events,
            ..
        } = won(&bus, Role::Service).await;
        assert!(has_owner(&bus).await, "name acquired while still Starting");
        let c = bus.connect().await;
        let call = tokio::spawn(async move { forward(&c, &["https://a/".into()]).await });
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(!call.is_finished(), "no reply before Ready");
        assert!(events.try_recv().is_err(), "no work before Ready");
        readiness.mark_ready();
        call.await.unwrap().unwrap();
        assert_eq!(
            events.recv().await,
            Some(BusEvent::Open {
                uris: vec!["https://a/".into()],
            })
        );
    })
    .await;
}

#[tokio::test]
async fn losing_starter_with_uris_forwards_its_uris() {
    let bus = support::PrivateBus::start();
    bounded("losing_starter_with_uris_forwards_its_uris", async {
        let Daemon {
            conn: _conn,
            readiness,
            mut events,
            ..
        } = won(&bus, Role::Service).await;
        readiness.mark_ready();
        let out = elect(
            builder(&bus),
            Role::Starter {
                uris: vec!["https://b/ x?y=%20#z".into()],
                token: Some("tok".into()),
            },
        )
        .await
        .unwrap();
        assert!(matches!(out, Outcome::Forwarded));
        assert_eq!(
            events.recv().await,
            Some(BusEvent::Open {
                uris: vec!["https://b/ x?y=%20#z".into()],
            })
        );
    })
    .await;
}

#[tokio::test]
async fn losing_service_sends_nothing() {
    let bus = support::PrivateBus::start();
    bounded("losing_service_sends_nothing", async {
        let Daemon {
            conn: _conn,
            readiness,
            mut events,
            ..
        } = won(&bus, Role::Service).await;
        readiness.mark_ready();
        assert!(matches!(
            elect(builder(&bus), Role::Service).await.unwrap(),
            Outcome::LostQuietly
        ));
        assert!(
            tokio::time::timeout(Duration::from_millis(300), events.recv())
                .await
                .is_err(),
            "no Open([]) or Activate"
        );
    })
    .await;
}

#[tokio::test]
async fn losing_starter_without_uris_asks_the_owner_for_settings_with_its_token() {
    let bus = support::PrivateBus::start();
    bounded(
        "losing_starter_without_uris_asks_the_owner_for_settings_with_its_token",
        async {
            let Daemon {
                conn: _conn,
                readiness,
                mut events,
                ..
            } = won(&bus, Role::Service).await;
            readiness.mark_ready();
            let out = elect(
                builder(&bus),
                Role::Starter {
                    uris: vec![],
                    token: Some("tok".into()),
                },
            )
            .await
            .unwrap();
            assert!(matches!(out, Outcome::Forwarded));
            assert_eq!(
                events.recv().await,
                Some(BusEvent::Activate {
                    token: Some("tok".into())
                })
            );
            assert!(
                tokio::time::timeout(Duration::from_millis(300), events.recv())
                    .await
                    .is_err(),
                "exactly one Activate"
            );
        },
    )
    .await;
}

#[tokio::test]
async fn losing_starter_without_uris_fails_when_the_owner_answers_with_an_error() {
    let bus = support::PrivateBus::start();
    bounded(
        "losing_starter_without_uris_fails_when_the_owner_answers_with_an_error",
        async {
            let Daemon {
                conn: _conn,
                readiness,
                events: _events,
                ..
            } = won(&bus, Role::Service).await;
            drop(readiness);
            let lost = elect(
                builder(&bus),
                Role::Starter {
                    uris: vec![],
                    token: None,
                },
            )
            .await;
            let Err(ElectError::Forward(error)) = lost else {
                panic!("the owner answered, yet the starter did not fail");
            };
            assert!(
                error.to_string().contains("signpost startup failed"),
                "{error}"
            );
        },
    )
    .await;
}

#[tokio::test]
async fn non_service_winner_processes_its_own_uris_once_after_ready() {
    let bus = support::PrivateBus::start();
    bounded(
        "non_service_winner_processes_its_own_uris_once_after_ready",
        async {
            let role = Role::Starter {
                uris: vec!["https://own/".into()],
                token: Some("own".into()),
            };
            let Daemon {
                conn: _conn,
                readiness,
                mut events,
                ..
            } = won(&bus, role).await;
            assert!(
                tokio::time::timeout(Duration::from_millis(200), events.recv())
                    .await
                    .is_err(),
                "nothing before Ready"
            );
            readiness.mark_ready();
            assert_eq!(
                events.recv().await,
                Some(BusEvent::Open {
                    uris: vec!["https://own/".into()],
                })
            );
            assert!(
                tokio::time::timeout(Duration::from_millis(300), events.recv())
                    .await
                    .is_err(),
                "exactly once"
            );
        },
    )
    .await;
}

/// The starter's own links take their room at the election, so calls that come while Signpost starts cannot
/// fill it first; a call that does not fit is refused at once instead of waiting for Ready.
#[tokio::test]
async fn a_winning_starters_own_links_are_counted_before_any_call_is() {
    let bus = support::PrivateBus::start();
    bounded(
        "a_winning_starters_own_links_are_counted_before_any_call_is",
        async {
            let role = Role::Starter {
                uris: vec!["https://own/1".into(), "https://own/2".into()],
                token: None,
            };
            let Daemon {
                conn: _conn,
                readiness,
                mut events,
                backlog,
            } = won(&bus, role).await;
            assert_eq!(backlog.waiting(), 2, "counted at the election");
            let c = bus.connect().await;
            let fill: Vec<String> = (0..MAX_WAITING_LINKS - 1)
                .map(|n| format!("https://a/{n}"))
                .collect();
            let refused = tokio::time::timeout(Duration::from_secs(1), forward(&c, &fill))
                .await
                .expect("refused while starting, not held until Ready");
            assert!(refused.is_err(), "{refused:?}");
            assert_eq!(backlog.waiting(), 2);
            readiness.mark_ready();
            assert!(
                matches!(events.recv().await, Some(BusEvent::Open { uris, .. }) if uris.len() == 2)
            );
            assert_eq!(backlog.waiting(), 2, "counted once");
        },
    )
    .await;
}

#[tokio::test]
async fn non_service_winner_without_uris_asks_for_settings_once_after_ready() {
    let bus = support::PrivateBus::start();
    bounded(
        "non_service_winner_without_uris_asks_for_settings_once_after_ready",
        async {
            let role = Role::Starter {
                uris: vec![],
                token: Some("own".into()),
            };
            let Daemon {
                conn: _conn,
                readiness,
                mut events,
                backlog,
            } = won(&bus, role).await;
            assert!(
                tokio::time::timeout(Duration::from_millis(200), events.recv())
                    .await
                    .is_err(),
                "nothing before Ready"
            );
            readiness.mark_ready();
            assert_eq!(
                events.recv().await,
                Some(BusEvent::Activate {
                    token: Some("own".into())
                })
            );
            assert!(
                tokio::time::timeout(Duration::from_millis(300), events.recv())
                    .await
                    .is_err(),
                "exactly once"
            );
            assert_eq!(backlog.waiting(), 0, "an Activate waits for no link");
        },
    )
    .await;
}

#[tokio::test]
async fn failed_startup_errors_a_pending_call_and_releases_the_name() {
    let bus = support::PrivateBus::start();
    bounded(
        "failed_startup_errors_a_pending_call_and_releases_the_name",
        async {
            let daemon = won(&bus, Role::Service).await;
            let c = bus.connect().await;
            let call = tokio::spawn(async move { forward(&c, &["https://p/".into()]).await });
            tokio::time::sleep(Duration::from_millis(150)).await;
            assert!(!call.is_finished(), "call is pending on the barrier");
            tokio::time::timeout(Duration::from_secs(5), daemon.fail())
                .await
                .expect("settlement completes");
            let err = tokio::time::timeout(Duration::from_secs(1), call)
                .await
                .expect("caller answered")
                .unwrap()
                .unwrap_err();
            assert!(err.to_string().contains("signpost startup failed"), "{err}");
            assert!(!has_owner(&bus).await);
        },
    )
    .await;
}
