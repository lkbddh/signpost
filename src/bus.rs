use std::collections::HashMap;
use std::hash::BuildHasher;
use std::time::Duration;

use tokio::sync::{mpsc, watch};
use zbus::fdo;
use zbus::zvariant::OwnedValue;

use crate::picker::{Backlog, MAX_WAITING_LINKS};

pub const APP_ID: &str = "com.lkbddh.signpost";
pub const APP_PATH: &str = "/com/lkbddh/signpost";
/// A call that arrives during startup waits at most this long for Ready/Failed.
pub const ADMISSION_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a losing starter waits for the running Signpost to take its links: past [`ADMISSION_TIMEOUT`], so an
/// owner that is still starting answers first.
const FORWARD_TIMEOUT: Duration = Duration::from_secs(35);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Startup {
    Starting,
    Ready,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BusEvent {
    /// Links to present. The picker asks for a token of its own for each launch, so the one they came with is not
    /// kept.
    Open {
        uris: Vec<String>,
    },
    Activate {
        token: Option<String>,
    },
}

impl BusEvent {
    /// How many links the event asks to present.
    fn links(&self) -> usize {
        match self {
            Self::Open { uris, .. } => uris.len(),
            Self::Activate { .. } => 0,
        }
    }
}

#[must_use]
pub fn token_from<S: BuildHasher>(
    platform_data: &HashMap<String, OwnedValue, S>,
) -> Option<String> {
    ["activation-token", "desktop-startup-id"]
        .iter()
        .find_map(|k| {
            // Borrow, never clone: cloning an fd-valued `OwnedValue` dups the descriptor and can panic.
            platform_data
                .get(*k)
                .and_then(|v| <&str>::try_from(v).ok())
                .map(str::to_owned)
        })
}

pub struct ApplicationIface {
    state: watch::Receiver<Startup>,
    events: mpsc::Sender<BusEvent>,
    backlog: Backlog,
}

impl ApplicationIface {
    #[must_use]
    pub fn new(
        state: watch::Receiver<Startup>,
        events: mpsc::Sender<BusEvent>,
        backlog: Backlog,
    ) -> Self {
        Self {
            state,
            events,
            backlog,
        }
    }

    /// Readiness barrier: no work and no success reply before Ready.
    async fn admit(&self) -> fdo::Result<()> {
        let mut state = self.state.clone();
        let outcome = tokio::time::timeout(
            ADMISSION_TIMEOUT,
            state.wait_for(|s| *s != Startup::Starting),
        )
        .await;
        match outcome {
            Ok(Ok(s)) if *s == Startup::Ready => Ok(()),
            _ => Err(fdo::Error::Failed("signpost startup failed".into())),
        }
    }

    /// The links are counted before the call waits for Ready or for room, so a flood is refused instead of
    /// held; they are given back if the call fails or is dropped while it waits.
    async fn deliver(&self, event: BusEvent) -> fdo::Result<()> {
        let links = event.links();
        if !self.backlog.try_add(links) {
            return Err(fdo::Error::LimitsExceeded(
                "too many links are waiting".into(),
            ));
        }
        let counted = Counted {
            backlog: &self.backlog,
            links,
        };
        self.admit().await?;
        let slot = self
            .events
            .reserve()
            .await
            .map_err(|_| fdo::Error::Failed("signpost is shutting down".into()))?;
        counted.keep();
        slot.send(event);
        Ok(())
    }
}

/// Links counted in the backlog that go back unless the event they belong to is queued.
struct Counted<'a> {
    backlog: &'a Backlog,
    links: usize,
}

impl Counted<'_> {
    fn keep(self) {
        std::mem::forget(self);
    }
}

impl Drop for Counted<'_> {
    fn drop(&mut self) {
        self.backlog.release(self.links);
    }
}

#[zbus::interface(name = "org.freedesktop.Application")]
impl ApplicationIface {
    async fn activate(&self, platform_data: HashMap<String, OwnedValue>) -> fdo::Result<()> {
        self.deliver(BusEvent::Activate {
            token: token_from(&platform_data),
        })
        .await
    }

    async fn open(
        &self,
        uris: Vec<String>,
        platform_data: HashMap<String, OwnedValue>,
    ) -> fdo::Result<()> {
        // Its token goes: the picker asks for one of its own for each launch.
        let _ = platform_data;
        self.deliver(BusEvent::Open { uris }).await
    }

    async fn activate_action(
        &self,
        action_name: String,
        parameter: Vec<OwnedValue>,
        platform_data: HashMap<String, OwnedValue>,
    ) -> fdo::Result<()> {
        let _ = (parameter, platform_data);
        self.admit().await?;
        Err(fdo::Error::NotSupported(format!(
            "signpost has no action “{action_name}”"
        )))
    }
}

/// The links of a starter that the waiting limit admits, and how many it refuses.
#[must_use]
pub fn admit_starter_links(mut uris: Vec<String>) -> (Vec<String>, usize) {
    let refused = uris.len().saturating_sub(MAX_WAITING_LINKS);
    uris.truncate(MAX_WAITING_LINKS);
    (uris, refused)
}

/// How this process was started; decides what a losing election forwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    /// D-Bus activation (`--service`): a loss forwards nothing.
    Service,
    /// A user launch: a loss forwards its links, a win processes them once after Ready. With none, it stands
    /// for the launcher's exec, which sends no `Activate`, and asks for the settings window the same way.
    Starter {
        uris: Vec<String>,
        /// The activation token it was started with, which raises the settings window; links go without it.
        token: Option<String>,
    },
}

/// Held until initialization has succeeded; `mark_ready` is the only way to publish Ready.
pub struct Readiness {
    state: watch::Sender<Startup>,
    tx: mpsc::Sender<BusEvent>,
    /// The starter's own event, whose links the election counted in `backlog`.
    own: Option<BusEvent>,
    backlog: Backlog,
}

impl Readiness {
    /// Publish Ready, then inject the starter's own event once.
    ///
    /// The queue slot is reserved while still Starting, when nothing has been admitted and the
    /// channel is empty, so a call admitted by Ready cannot take the slot and the links are never
    /// lost to capacity. They are sent only after Ready is published.
    pub fn mark_ready(self) {
        let reserved = self.own.map(|own| (own, self.tx.try_reserve()));
        self.state.send_replace(Startup::Ready);
        match reserved {
            Some((own, Ok(slot))) => slot.send(own),
            Some((own, Err(e))) => {
                self.backlog.release(own.links());
                tracing::error!(error = %e, "could not queue the starter's own links");
            }
            None => {}
        }
    }
}

/// The election winner. Still `Starting`: call `readiness.mark_ready()` once the app exists.
pub struct Daemon {
    pub conn: zbus::Connection,
    pub readiness: Readiness,
    pub events: mpsc::Receiver<BusEvent>,
    /// Shared with the interface; whoever presents or drops a link frees its room.
    pub backlog: Backlog,
}

pub enum Outcome {
    Won(Daemon),
    Forwarded,
    LostQuietly,
}

#[derive(Debug, thiserror::Error)]
pub enum ElectError {
    #[error("session bus unavailable: {0}")]
    Bus(#[from] zbus::Error),
    #[error("forwarding to the running Signpost failed: {0}")]
    Forward(zbus::Error),
    #[error("the running Signpost did not answer within {} seconds", FORWARD_TIMEOUT.as_secs())]
    Unanswered,
}

/// Calls `Open(uris, {})` on the running Signpost.
///
/// # Errors
/// Fails if the call cannot be made or the owner answers with an error.
pub async fn forward(conn: &zbus::Connection, uris: &[String]) -> zbus::Result<()> {
    let proxy = application_proxy(conn).await?;
    proxy
        .call_method("Open", &(uris, platform_data(None)))
        .await
        .map(|_| ())
}

/// Calls `Activate({"activation-token": token})` on the running Signpost.
///
/// # Errors
/// Fails if the call cannot be made or the owner answers with an error.
pub async fn activate(conn: &zbus::Connection, token: Option<&str>) -> zbus::Result<()> {
    let proxy = application_proxy(conn).await?;
    proxy
        .call_method("Activate", &(platform_data(token),))
        .await
        .map(|_| ())
}

async fn application_proxy(conn: &zbus::Connection) -> zbus::Result<zbus::Proxy<'static>> {
    zbus::Proxy::new(conn, APP_ID, APP_PATH, "org.freedesktop.Application").await
}

fn platform_data(token: Option<&str>) -> HashMap<&str, zbus::zvariant::Value<'_>> {
    token
        .map(|token| ("activation-token", token.into()))
        .into_iter()
        .collect()
}

/// Exports the interface, then requests `com.lkbddh.signpost` with `DoNotQueue`.
///
/// # Errors
/// `Bus` if the bus is unavailable or the name request fails for any reason but the name being
/// taken; `Forward` if a losing starter could not hand its links to the owner.
pub async fn elect(
    builder: zbus::connection::Builder<'static>,
    role: Role,
) -> Result<Outcome, ElectError> {
    let (state, state_rx) = watch::channel(Startup::Starting);
    let (tx, events) = mpsc::channel(64);
    let backlog = Backlog::default();
    // Before the name is taken, so no call that comes while Signpost starts takes their room.
    if let Role::Starter { uris, .. } = &role {
        backlog.add(uris.len());
    }
    let conn = builder
        .serve_at(
            APP_PATH,
            ApplicationIface::new(state_rx, tx.clone(), backlog.clone()),
        )?
        .build()
        .await?;
    // zbus reports `RequestNameReply::Exists` (a DoNotQueue loss) as `Err(NameTaken)`.
    match conn
        .request_name_with_flags(APP_ID, zbus::fdo::RequestNameFlags::DoNotQueue.into())
        .await
    {
        Ok(_) => {
            // Stay Starting: Ready is published by `Readiness::mark_ready` once the app exists.
            let own = match role {
                Role::Service => None,
                Role::Starter { uris, token } if uris.is_empty() => {
                    Some(BusEvent::Activate { token })
                }
                Role::Starter { uris, .. } => Some(BusEvent::Open { uris }),
            };
            Ok(Outcome::Won(Daemon {
                conn,
                readiness: Readiness {
                    state,
                    tx,
                    own,
                    backlog: backlog.clone(),
                },
                events,
                backlog,
            }))
        }
        Err(zbus::Error::NameTaken) => {
            state.send_replace(Startup::Failed);
            match role {
                Role::Service => return Ok(Outcome::LostQuietly),
                Role::Starter { uris, token } if uris.is_empty() => {
                    handed_over(activate(&conn, token.as_deref())).await?;
                }
                Role::Starter { uris, .. } => {
                    handed_over(forward(&conn, &uris)).await?;
                }
            }
            Ok(Outcome::Forwarded)
        }
        Err(e) => {
            state.send_replace(Startup::Failed);
            conn.graceful_shutdown().await;
            Err(ElectError::Bus(e))
        }
    }
}

/// Waits for `handover` to the running Signpost, which may never answer, until [`FORWARD_TIMEOUT`].
async fn handed_over(handover: impl Future<Output = zbus::Result<()>>) -> Result<(), ElectError> {
    tokio::time::timeout(FORWARD_TIMEOUT, handover)
        .await
        .map_err(|_| ElectError::Unanswered)?
        .map_err(ElectError::Forward)
}

impl Daemon {
    /// Startup failure settlement: publish Failed (admitted calls get errors), release the name,
    /// then `graceful_shutdown`, which waits until every in-flight call — and its reply — has completed.
    pub async fn fail(self) {
        self.readiness.state.send_replace(Startup::Failed);
        let _ = self.conn.release_name(APP_ID).await;
        drop(self.readiness);
        drop(self.events);
        self.conn.graceful_shutdown().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Wake, Waker};
    use zbus::zvariant::{Fd, Value};

    const TEST_TIMEOUT: Duration = Duration::from_secs(10);

    /// Bounds a test body so a missing reply fails the test instead of hanging it.
    async fn bounded<T>(name: &str, body: impl Future<Output = T>) -> T {
        tokio::time::timeout(TEST_TIMEOUT, body)
            .await
            .unwrap_or_else(|_| panic!("{name} timed out"))
    }

    async fn pair(
        state: watch::Receiver<Startup>,
        tx: mpsc::Sender<BusEvent>,
    ) -> (zbus::Connection, zbus::Connection) {
        pair_counting(state, tx, Backlog::default()).await
    }

    async fn pair_counting(
        state: watch::Receiver<Startup>,
        tx: mpsc::Sender<BusEvent>,
        backlog: Backlog,
    ) -> (zbus::Connection, zbus::Connection) {
        let (a, b) = tokio::net::UnixStream::pair().unwrap();
        let guid = zbus::Guid::generate();
        let server = zbus::connection::Builder::unix_stream(a)
            .server(guid)
            .unwrap()
            .p2p()
            .serve_at(APP_PATH, ApplicationIface::new(state, tx, backlog))
            .unwrap()
            .build();
        let client = zbus::connection::Builder::unix_stream(b).p2p().build();
        let (s, c) = tokio::join!(server, client);
        (s.unwrap(), c.unwrap())
    }

    async fn call_open(
        client: &zbus::Connection,
        uri: &str,
        token: Option<&str>,
    ) -> zbus::Result<()> {
        let proxy =
            zbus::Proxy::new(client, APP_ID, APP_PATH, "org.freedesktop.Application").await?;
        proxy
            .call_method("Open", &(vec![uri], platform_data(token)))
            .await
            .map(|_| ())
    }

    async fn call_open_links(client: &zbus::Connection, count: usize) -> zbus::Result<()> {
        let proxy =
            zbus::Proxy::new(client, APP_ID, APP_PATH, "org.freedesktop.Application").await?;
        let uris: Vec<String> = (0..count).map(|n| format!("https://a/{n}")).collect();
        proxy
            .call_method("Open", &(uris, platform_data(None)))
            .await
            .map(|_| ())
    }

    async fn call_activate(client: &zbus::Connection, token: Option<&str>) -> zbus::Result<()> {
        let proxy =
            zbus::Proxy::new(client, APP_ID, APP_PATH, "org.freedesktop.Application").await?;
        proxy
            .call_method("Activate", &(platform_data(token),))
            .await
            .map(|_| ())
    }

    /// The picker asks for a token of its own for each launch, so the one the links came with goes.
    #[tokio::test]
    async fn ready_open_delivers_its_links() {
        bounded("ready_open_delivers_its_links", async {
            let (_s, state) = watch::channel(Startup::Ready);
            let (tx, mut rx) = mpsc::channel(4);
            let (_server, client) = pair(state, tx).await;
            call_open(&client, "https://a/", Some("tok")).await.unwrap();
            assert_eq!(
                rx.recv().await,
                Some(BusEvent::Open {
                    uris: vec!["https://a/".into()],
                })
            );
        })
        .await;
    }

    #[tokio::test]
    async fn ready_activate_delivers_event_with_token() {
        bounded("ready_activate_delivers_event_with_token", async {
            let (_s, state) = watch::channel(Startup::Ready);
            let (tx, mut rx) = mpsc::channel(4);
            let (_server, client) = pair(state, tx).await;
            call_activate(&client, Some("tok")).await.unwrap();
            assert_eq!(
                rx.recv().await,
                Some(BusEvent::Activate {
                    token: Some("tok".into())
                })
            );
        })
        .await;
    }

    #[tokio::test]
    async fn starting_call_waits_until_ready() {
        bounded("starting_call_waits_until_ready", async {
            let (set, state) = watch::channel(Startup::Starting);
            let (tx, mut rx) = mpsc::channel(4);
            let (_server, client) = pair(state, tx).await;
            let call = tokio::spawn(async move { call_open(&client, "https://b/", None).await });
            tokio::time::sleep(Duration::from_millis(150)).await;
            assert!(rx.try_recv().is_err(), "no work before Ready");
            assert!(!call.is_finished(), "no reply before Ready");
            set.send(Startup::Ready).unwrap();
            call.await.unwrap().unwrap();
            assert_eq!(
                rx.recv().await,
                Some(BusEvent::Open {
                    uris: vec!["https://b/".into()],
                })
            );
        })
        .await;
    }

    #[tokio::test]
    async fn failed_startup_errors_and_does_no_work() {
        bounded("failed_startup_errors_and_does_no_work", async {
            let (set, state) = watch::channel(Startup::Starting);
            let (tx, mut rx) = mpsc::channel(4);
            let (_server, client) = pair(state, tx).await;
            let call = tokio::spawn(async move { call_open(&client, "https://c/", None).await });
            tokio::time::sleep(Duration::from_millis(100)).await;
            set.send(Startup::Failed).unwrap();
            let err = call.await.unwrap().unwrap_err();
            assert!(err.to_string().contains("signpost startup failed"), "{err}");
            assert!(rx.try_recv().is_err());
        })
        .await;
    }

    #[tokio::test(start_paused = true)]
    async fn admission_timeout_errors_and_does_no_work() {
        let (_set, state) = watch::channel(Startup::Starting);
        let (tx, mut rx) = mpsc::channel(4);
        let iface = ApplicationIface::new(state, tx, Backlog::default());
        let started = tokio::time::Instant::now();
        // Virtual time: the outer bound only trips if `admit` never gives up on its own.
        tokio::time::timeout(ADMISSION_TIMEOUT * 2, async {
            let err = iface.admit().await.unwrap_err();
            assert!(err.to_string().contains("signpost startup failed"), "{err}");
            assert!(started.elapsed() >= ADMISSION_TIMEOUT, "gave up early");
            let err = iface
                .deliver(BusEvent::Activate { token: None })
                .await
                .unwrap_err();
            assert!(err.to_string().contains("signpost startup failed"), "{err}");
            assert!(rx.try_recv().is_err(), "no work after a failed admission");
        })
        .await
        .expect("admission_timeout_errors_and_does_no_work timed out");
    }

    #[tokio::test]
    async fn dropped_startup_sender_errors_and_does_no_work() {
        bounded("dropped_startup_sender_errors_and_does_no_work", async {
            let (set, state) = watch::channel(Startup::Starting);
            let (tx, mut rx) = mpsc::channel(4);
            let iface = ApplicationIface::new(state, tx, Backlog::default());
            drop(set);
            let err = iface.admit().await.unwrap_err();
            assert!(err.to_string().contains("signpost startup failed"), "{err}");
            let err = iface
                .deliver(BusEvent::Activate { token: None })
                .await
                .unwrap_err();
            assert!(err.to_string().contains("signpost startup failed"), "{err}");
            assert!(rx.try_recv().is_err());
        })
        .await;
    }

    #[tokio::test]
    async fn closed_event_receiver_reports_shutting_down() {
        bounded("closed_event_receiver_reports_shutting_down", async {
            let (_s, state) = watch::channel(Startup::Ready);
            let (tx, rx) = mpsc::channel(4);
            let (_server, client) = pair(state, tx).await;
            drop(rx);
            let err = call_open(&client, "https://d/", None).await.unwrap_err();
            assert!(
                err.to_string().contains("signpost is shutting down"),
                "{err}"
            );
        })
        .await;
    }

    /// A ready interface with room for four events, and what it counts.
    async fn counting() -> (
        zbus::Connection,
        mpsc::Receiver<BusEvent>,
        Backlog,
        zbus::Connection,
    ) {
        let (_s, state) = watch::channel(Startup::Ready);
        let (tx, rx) = mpsc::channel(4);
        let backlog = Backlog::default();
        let (server, client) = pair_counting(state, tx, backlog.clone()).await;
        (client, rx, backlog, server)
    }

    #[tokio::test]
    async fn open_at_the_cap_is_refused_and_queues_nothing() {
        bounded("open_at_the_cap_is_refused_and_queues_nothing", async {
            let (client, mut rx, backlog, _server) = counting().await;
            call_open_links(&client, MAX_WAITING_LINKS).await.unwrap();
            assert!(rx.try_recv().is_ok(), "the first call is delivered");
            let err = call_open(&client, "https://over/", None).await.unwrap_err();
            assert!(err.to_string().contains("LimitsExceeded"), "{err}");
            assert!(
                err.to_string().contains("too many links are waiting"),
                "{err}"
            );
            assert!(rx.try_recv().is_err(), "nothing was enqueued");
            assert_eq!(backlog.waiting(), MAX_WAITING_LINKS, "nothing was counted");
        })
        .await;
    }

    #[tokio::test]
    async fn a_call_that_would_pass_the_cap_is_refused_whole() {
        bounded("a_call_that_would_pass_the_cap_is_refused_whole", async {
            let (client, mut rx, backlog, _server) = counting().await;
            call_open_links(&client, MAX_WAITING_LINKS - 4)
                .await
                .unwrap();
            rx.try_recv().unwrap();
            call_open_links(&client, 5).await.unwrap_err();
            assert!(rx.try_recv().is_err(), "none of the five was enqueued");
            assert_eq!(backlog.waiting(), MAX_WAITING_LINKS - 4);
            call_open_links(&client, 4).await.unwrap();
            assert_eq!(backlog.waiting(), MAX_WAITING_LINKS, "four still fit");
        })
        .await;
    }

    #[tokio::test]
    async fn presenting_links_makes_room_for_the_next_call() {
        bounded("presenting_links_makes_room_for_the_next_call", async {
            let (client, _rx, backlog, _server) = counting().await;
            call_open_links(&client, MAX_WAITING_LINKS).await.unwrap();
            call_open(&client, "https://over/", None).await.unwrap_err();
            backlog.release(1);
            call_open(&client, "https://fits/", None).await.unwrap();
            assert_eq!(backlog.waiting(), MAX_WAITING_LINKS);
        })
        .await;
    }

    #[tokio::test]
    async fn activate_is_never_refused_for_the_backlog() {
        bounded("activate_is_never_refused_for_the_backlog", async {
            let (client, mut rx, backlog, _server) = counting().await;
            backlog.add(MAX_WAITING_LINKS + 3);
            call_activate(&client, None).await.unwrap();
            assert_eq!(rx.recv().await, Some(BusEvent::Activate { token: None }));
            assert_eq!(backlog.waiting(), MAX_WAITING_LINKS + 3);
        })
        .await;
    }

    #[tokio::test]
    async fn links_that_cannot_be_delivered_are_not_counted() {
        bounded("links_that_cannot_be_delivered_are_not_counted", async {
            let (client, rx, backlog, _server) = counting().await;
            drop(rx);
            call_open(&client, "https://d/", None).await.unwrap_err();
            assert_eq!(backlog.waiting(), 0);
        })
        .await;
    }

    fn open_links(count: usize) -> BusEvent {
        BusEvent::Open {
            uris: (0..count).map(|n| format!("https://a/{n}")).collect(),
        }
    }

    /// A ready interface over a channel with one slot, and what it counts.
    fn one_slot() -> (ApplicationIface, mpsc::Receiver<BusEvent>, Backlog) {
        let (_s, state) = watch::channel(Startup::Ready);
        let (tx, rx) = mpsc::channel(1);
        let backlog = Backlog::default();
        (
            ApplicationIface::new(state, tx, backlog.clone()),
            rx,
            backlog,
        )
    }

    #[tokio::test(start_paused = true)]
    async fn an_over_cap_call_is_refused_at_once_while_the_channel_is_full() {
        let (iface, _rx, backlog) = one_slot();
        iface.deliver(open_links(MAX_WAITING_LINKS)).await.unwrap();
        let refused = tokio::time::timeout(Duration::from_secs(1), iface.deliver(open_links(1)))
            .await
            .expect("the call waited for room instead of being refused");
        let err = refused.unwrap_err();
        assert!(err.to_string().contains("LimitsExceeded"), "{err}");
        assert_eq!(backlog.waiting(), MAX_WAITING_LINKS, "nothing was counted");
    }

    #[tokio::test(start_paused = true)]
    async fn links_waiting_for_room_are_counted_and_a_dropped_call_gives_them_back() {
        let (iface, _rx, backlog) = one_slot();
        iface.deliver(open_links(1)).await.unwrap();
        let mut call = Box::pin(iface.deliver(open_links(2)));
        let waiting = call.as_mut().poll(&mut Context::from_waker(Waker::noop()));
        assert!(waiting.is_pending(), "the channel is full");
        assert_eq!(backlog.waiting(), 3, "the waiting call holds its links");
        drop(call);
        assert_eq!(backlog.waiting(), 1, "a dropped call gives its links back");
    }

    /// A ready interface would count them on arrival too; while Signpost starts, the calls wait for Ready.
    fn starting() -> (
        watch::Sender<Startup>,
        ApplicationIface,
        mpsc::Receiver<BusEvent>,
        Backlog,
    ) {
        let (set, state) = watch::channel(Startup::Starting);
        let (tx, rx) = mpsc::channel(4);
        let backlog = Backlog::default();
        (
            set,
            ApplicationIface::new(state, tx, backlog.clone()),
            rx,
            backlog,
        )
    }

    #[tokio::test(start_paused = true)]
    async fn a_call_over_the_cap_while_signpost_starts_is_refused_at_once() {
        let (_set, iface, _rx, backlog) = starting();
        backlog.add(MAX_WAITING_LINKS - 1);
        let refused = tokio::time::timeout(Duration::from_secs(1), iface.deliver(open_links(2)))
            .await
            .expect("the call waited for Ready instead of being refused");
        let err = refused.unwrap_err();
        assert!(err.to_string().contains("LimitsExceeded"), "{err}");
        assert_eq!(
            backlog.waiting(),
            MAX_WAITING_LINKS - 1,
            "nothing was counted"
        );
    }

    #[tokio::test]
    async fn links_waiting_for_startup_are_counted_and_given_back_when_it_fails() {
        let (set, iface, mut rx, backlog) = starting();
        let mut call = Box::pin(iface.deliver(open_links(2)));
        let waiting = call.as_mut().poll(&mut Context::from_waker(Waker::noop()));
        assert!(waiting.is_pending(), "Signpost is starting");
        assert_eq!(backlog.waiting(), 2, "the waiting call holds its links");
        set.send_replace(Startup::Failed);
        call.await.unwrap_err();
        assert_eq!(backlog.waiting(), 0, "a failed startup gives them back");
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn a_starter_the_running_signpost_never_answers_gives_up_after_the_deadline() {
        const { assert!(FORWARD_TIMEOUT.as_secs() > ADMISSION_TIMEOUT.as_secs()) };
        let started = tokio::time::Instant::now();
        let outcome = handed_over(std::future::pending()).await;
        assert!(
            matches!(outcome, Err(ElectError::Unanswered)),
            "{outcome:?}"
        );
        assert!(started.elapsed() >= FORWARD_TIMEOUT);
    }

    #[tokio::test]
    async fn links_the_channel_had_no_room_for_are_not_left_counted() {
        bounded(
            "links_the_channel_had_no_room_for_are_not_left_counted",
            async {
                let (state, _state_rx) = watch::channel(Startup::Starting);
                let (tx, rx) = mpsc::channel(1);
                let backlog = Backlog::default();
                // The election counted them.
                backlog.add(own_links().links());
                drop(rx);
                Readiness {
                    state,
                    tx,
                    own: Some(own_links()),
                    backlog: backlog.clone(),
                }
                .mark_ready();
                assert_eq!(backlog.waiting(), 0);
            },
        )
        .await;
    }

    #[test]
    fn a_starter_is_admitted_up_to_the_limit_and_refused_the_rest() {
        let links = |count: usize| {
            (0..count)
                .map(|n| format!("https://a/{n}"))
                .collect::<Vec<_>>()
        };
        assert_eq!(admit_starter_links(links(0)), (links(0), 0));
        assert_eq!(
            admit_starter_links(links(MAX_WAITING_LINKS)),
            (links(MAX_WAITING_LINKS), 0)
        );
        let (admitted, refused) = admit_starter_links(links(MAX_WAITING_LINKS + 6));
        assert_eq!(
            admitted,
            links(MAX_WAITING_LINKS),
            "the first ones are kept"
        );
        assert_eq!(refused, 6);
    }

    #[tokio::test]
    async fn activate_action_is_not_supported() {
        bounded("activate_action_is_not_supported", async {
            let (_s, state) = watch::channel(Startup::Ready);
            let (tx, _rx) = mpsc::channel(4);
            let (_server, client) = pair(state, tx).await;
            let proxy = zbus::Proxy::new(&client, APP_ID, APP_PATH, "org.freedesktop.Application")
                .await
                .unwrap();
            let pd: HashMap<&str, Value<'_>> = HashMap::new();
            let err = proxy
                .call_method("ActivateAction", &("x", Vec::<Value<'_>>::new(), pd))
                .await
                .unwrap_err();
            assert!(err.to_string().contains("NotSupported"), "{err}");
        })
        .await;
    }

    fn own_links() -> BusEvent {
        BusEvent::Open {
            uris: vec!["https://own/".into()],
        }
    }

    /// A waker that runs `hook` on the waking thread, inside the channel operation that wakes it.
    /// That makes the state at the moment a send or a publish becomes observable assertable.
    struct OnWake(Box<dyn Fn() + Send + Sync>);

    impl Wake for OnWake {
        fn wake(self: Arc<Self>) {
            (self.0)();
        }
    }

    fn waker(hook: impl Fn() + Send + Sync + 'static) -> Waker {
        Waker::from(Arc::new(OnWake(Box::new(hook))))
    }

    #[tokio::test]
    async fn mark_ready_publishes_ready_before_the_own_links_are_visible() {
        bounded(
            "mark_ready_publishes_ready_before_the_own_links_are_visible",
            async {
                let (state, state_rx) = watch::channel(Startup::Starting);
                let (tx, mut rx) = mpsc::channel(4);
                let at_wake = Arc::new(Mutex::new(None));
                let waker = waker({
                    let at_wake = Arc::clone(&at_wake);
                    move || {
                        at_wake.lock().unwrap().get_or_insert(*state_rx.borrow());
                    }
                });
                let mut recv = Box::pin(rx.recv());
                assert!(
                    recv.as_mut()
                        .poll(&mut Context::from_waker(&waker))
                        .is_pending()
                );
                Readiness {
                    state,
                    tx,
                    own: Some(own_links()),
                    backlog: Backlog::default(),
                }
                .mark_ready();
                assert_eq!(
                    *at_wake.lock().unwrap(),
                    Some(Startup::Ready),
                    "own links became visible before Ready"
                );
                assert_eq!(recv.await, Some(own_links()));
                assert_eq!(rx.recv().await, None, "exactly once");
            },
        )
        .await;
    }

    #[tokio::test]
    async fn own_links_are_not_lost_when_a_call_fills_the_channel_as_ready_is_published() {
        bounded(
            "own_links_are_not_lost_when_a_call_fills_the_channel_as_ready_is_published",
            async {
                let (state, mut state_rx) = watch::channel(Startup::Starting);
                let (tx, mut rx) = mpsc::channel(1);
                let late = tx.clone();
                // A call the barrier holds back, woken inside the publish; it sends at once, like `deliver`.
                let waker = waker({
                    let late = late.clone();
                    move || {
                        let _ = late.try_send(BusEvent::Activate { token: None });
                    }
                });
                let mut waiting = Box::pin(state_rx.wait_for(|s| *s == Startup::Ready));
                assert!(
                    waiting
                        .as_mut()
                        .poll(&mut Context::from_waker(&waker))
                        .is_pending()
                );
                Readiness {
                    state,
                    tx,
                    own: Some(own_links()),
                    backlog: Backlog::default(),
                }
                .mark_ready();
                assert_eq!(rx.try_recv(), Ok(own_links()), "own links lost to capacity");
                // The slot is free again for the call that was woken.
                late.send(BusEvent::Activate { token: None }).await.unwrap();
                assert_eq!(rx.recv().await, Some(BusEvent::Activate { token: None }));
            },
        )
        .await;
    }

    #[test]
    fn token_prefers_activation_token() {
        let mut pd = HashMap::new();
        pd.insert(
            "desktop-startup-id".to_owned(),
            OwnedValue::try_from(Value::from("old")).unwrap(),
        );
        assert_eq!(token_from(&pd), Some("old".into()));
        pd.insert(
            "activation-token".to_owned(),
            OwnedValue::try_from(Value::from("new")).unwrap(),
        );
        assert_eq!(token_from(&pd), Some("new".into()));
        assert_eq!(token_from(&HashMap::new()), None);
    }

    #[test]
    fn non_string_tokens_are_ignored() {
        let file = std::fs::File::open("/dev/null").unwrap();
        let non_strings = [
            OwnedValue::try_from(Value::from(7_u32)).unwrap(),
            OwnedValue::try_from(Value::Fd(Fd::from(&file))).unwrap(),
        ];
        for bad in non_strings {
            let mut pd = HashMap::new();
            pd.insert("activation-token".to_owned(), bad);
            assert_eq!(token_from(&pd), None);
            pd.insert(
                "desktop-startup-id".to_owned(),
                OwnedValue::try_from(Value::from("fallback")).unwrap(),
            );
            assert_eq!(token_from(&pd), Some("fallback".into()));
        }
    }
}
