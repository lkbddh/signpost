//! The app as `init` builds it, with a peer-to-peer connection in place of the session bus.
//!
//! Building the app reads libcosmic's configuration and the app index, so the tests that use it run again
//! in a child process whose home is a scratch directory (`headless::in_scratch_home`).

use std::sync::{Arc, Mutex};

use cosmic::Application as _;
use cosmic::app::{Core, Task};
use cosmic::iced::runtime::Action as RuntimeAction;
use cosmic::iced::runtime::platform_specific::Action as PlatformAction;
use cosmic::iced::runtime::platform_specific::wayland::Action as WaylandAction;
use cosmic::iced::runtime::platform_specific::wayland::activation::Action as ActivationAction;
use cosmic::iced::{window, window::Action as WindowAction};
use cosmic::surface::Action as Surface;
use futures_util::StreamExt;
use zbus::connection::Builder;

use super::{App, Flags, Inbox, Message};
use crate::host::Host;
use crate::picker::Backlog;
use crate::test_support::peer;

/// The windows a task asks the runtime to raise: by focus, and by an activation token.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Raised {
    pub(super) focused: Vec<window::Id>,
    pub(super) activated: Vec<(window::Id, String)>,
}

pub(super) struct World {
    pub(super) app: App,
    _peer: zbus::Connection,
    pub(super) runtime: tokio::runtime::Runtime,
}

impl World {
    pub(super) fn new() -> Self {
        Self::serving(Ok)
    }

    /// A world whose peer exports what `serve` adds to it.
    pub(super) fn serving(
        serve: impl FnOnce(Builder<'static>) -> zbus::Result<Builder<'static>>,
    ) -> Self {
        Self::on(Host::Native, serve)
    }

    /// A world on `host` whose peer exports what `serve` adds to it.
    pub(super) fn on(
        host: Host,
        serve: impl FnOnce(Builder<'static>) -> zbus::Result<Builder<'static>>,
    ) -> Self {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime");
        let (conn, peer) = runtime.block_on(peer::connect(serve));
        let flags = Flags {
            host,
            inbox: Inbox(Arc::new(Mutex::new(None))),
            conn,
            readiness: Mutex::new(None),
            backlog: Backlog::default(),
        };
        let (app, _) = App::init(Core::default(), flags);
        Self {
            app,
            _peer: peer,
            runtime,
        }
    }

    fn actions(&self, task: Task<Message>) -> Vec<RuntimeAction<cosmic::Action<Message>>> {
        let Some(stream) = cosmic::iced::runtime::task::into_stream(task) else {
            return Vec::new();
        };
        self.runtime.block_on(stream.collect())
    }

    /// The surface actions `task` sends libcosmic.
    pub(super) fn surface_actions(&self, task: Task<Message>) -> Vec<Surface<Message>> {
        self.actions(task)
            .into_iter()
            .filter_map(|action| {
                let RuntimeAction::Output(cosmic::Action::Surface(surface)) = action else {
                    return None;
                };
                Some(surface)
            })
            .collect()
    }

    /// The windows `task` asks the runtime to raise.
    pub(super) fn raised(&self, task: Task<Message>) -> Raised {
        let mut raised = Raised::default();
        for action in self.actions(task) {
            match action {
                RuntimeAction::Window(WindowAction::GainFocus(id)) => raised.focused.push(id),
                RuntimeAction::PlatformSpecific(PlatformAction::Wayland(
                    WaylandAction::Activation(ActivationAction::Activate { window, token }),
                )) => raised.activated.push((window, token)),
                _ => {}
            }
        }
        raised
    }
}
