//! Fake D-Bus services on a peer-to-peer connection, in place of a bus.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{Notify, mpsc};
use zbus::connection::Builder;
use zbus::fdo;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue};

/// A client connection to a peer that exports what `serve` adds to it, and the peer's own connection,
/// which has to stay alive as long as the client is used.
pub async fn connect(
    serve: impl FnOnce(Builder<'static>) -> zbus::Result<Builder<'static>>,
) -> (zbus::Connection, zbus::Connection) {
    let (a, b) = tokio::net::UnixStream::pair().expect("a socket pair");
    let server = Builder::unix_stream(a)
        .server(zbus::Guid::generate())
        .expect("a server")
        .p2p();
    let peer = serve(server).expect("the peer exports its objects").build();
    let client = Builder::unix_stream(b).p2p().build();
    let (peer, client) = tokio::join!(peer, client);
    (client.expect("a connection"), peer.expect("a peer"))
}

/// An `org.freedesktop.Application` that keeps its name and never answers `Open`; it says when one arrives.
pub struct SilentApp(pub mpsc::UnboundedSender<()>);

#[zbus::interface(name = "org.freedesktop.Application")]
impl SilentApp {
    async fn open(&self, uris: Vec<String>, platform_data: HashMap<String, OwnedValue>) {
        let _ = (uris, platform_data);
        let _ = self.0.send(());
        std::future::pending::<()>().await;
    }
}

/// An `org.freedesktop.Application` that answers `Open` at once.
pub struct AnsweringApp;

#[zbus::interface(name = "org.freedesktop.Application")]
impl AnsweringApp {
    #[expect(clippy::unused_self, reason = "zbus exports methods that take `&self`")]
    fn open(&self, uris: Vec<String>, platform_data: HashMap<String, OwnedValue>) {
        let _ = (uris, platform_data);
    }
}

/// Where systemd's manager is exported.
pub const SYSTEMD_PATH: &str = "/org/freedesktop/systemd1";

/// The job the manager queues for every `StartTransientUnit`, and the id it gives it.
pub const JOB: &str = "/org/freedesktop/systemd1/job/1";
const JOB_ID: u32 = 1;

/// One `StartTransientUnit` call as the manager received it.
#[derive(Debug)]
pub struct TransientUnit {
    pub name: String,
    pub mode: String,
    pub properties: Vec<(String, OwnedValue)>,
}

/// How the manager answers `StartTransientUnit`.
enum Answer {
    /// Runs the job, then replies.
    Start,
    /// Replies with the queued job and leaves its removal to the test, as when the job has yet to run.
    Queue,
    Refuse(&'static str),
    /// Answers once the test notifies it.
    Hold(Arc<Notify>),
}

/// The user's systemd manager, which records the transient units it is asked to start, and starts them,
/// queues them, refuses, or keeps the caller waiting.
pub struct FakeManager {
    asked: mpsc::UnboundedSender<TransientUnit>,
    answer: Answer,
}

impl FakeManager {
    pub fn starting(asked: mpsc::UnboundedSender<TransientUnit>) -> Self {
        Self {
            asked,
            answer: Answer::Start,
        }
    }

    /// Replies at once with the job it queued, and sends `JobRemoved` only when the test says ([`remove_job`]).
    pub fn queuing(asked: mpsc::UnboundedSender<TransientUnit>) -> Self {
        Self {
            asked,
            answer: Answer::Queue,
        }
    }

    pub fn refusing(asked: mpsc::UnboundedSender<TransientUnit>, reason: &'static str) -> Self {
        Self {
            asked,
            answer: Answer::Refuse(reason),
        }
    }

    /// Takes the call and answers it, starting the unit, once `release` is notified.
    pub fn holding(asked: mpsc::UnboundedSender<TransientUnit>, release: Arc<Notify>) -> Self {
        Self {
            asked,
            answer: Answer::Hold(release),
        }
    }

    /// Takes the call and never answers it.
    pub fn silent(asked: mpsc::UnboundedSender<TransientUnit>) -> Self {
        Self::holding(asked, Arc::new(Notify::new()))
    }
}

#[zbus::interface(name = "org.freedesktop.systemd1.Manager")]
impl FakeManager {
    async fn start_transient_unit(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        name: String,
        mode: String,
        properties: Vec<(String, OwnedValue)>,
        aux: Vec<(String, Vec<(String, OwnedValue)>)>,
    ) -> fdo::Result<OwnedObjectPath> {
        let _ = aux;
        let unit = name.clone();
        let _ = self.asked.send(TransientUnit {
            name,
            mode,
            properties,
        });
        let job = OwnedObjectPath::try_from(JOB).expect("a job path");
        match &self.answer {
            Answer::Start => {}
            Answer::Queue => return Ok(job),
            Answer::Refuse(reason) => return Err(fdo::Error::Failed((*reason).to_owned())),
            Answer::Hold(release) => release.notified().await,
        }
        // Sent before the reply, which is the order a job that runs at once can keep.
        Self::job_removed(&emitter, JOB_ID, job.as_ref().clone(), &unit, "done").await?;
        Ok(job)
    }

    #[zbus(signal)]
    async fn job_removed(
        emitter: &SignalEmitter<'_>,
        id: u32,
        job: ObjectPath<'_>,
        unit: &str,
        result: &str,
    ) -> zbus::Result<()>;
}

/// The manager's `JobRemoved` for `job`, which ended as `result` ("done" when the job did what it was for).
pub async fn remove_job(manager: &zbus::Connection, job: &str, result: &str) {
    let emitter = SignalEmitter::new(manager, SYSTEMD_PATH).expect("an emitter");
    let job = ObjectPath::try_from(job).expect("a job path");
    FakeManager::job_removed(&emitter, JOB_ID, job, "the unit", result)
        .await
        .expect("the signal is sent");
}
