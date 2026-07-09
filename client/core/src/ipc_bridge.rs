use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::events::bus::{Emitter, Error as EventError, Event, EventBus, EventChannel};
use crate::events::remote::{IpcListener, RemoteEventBus, RemoteSender};
use crate::module::auth::{LoginRequested, LoginResult, Logout, LogoutRequested, LogoutResult};
use crate::module::lifecycle::{
    ProcessStopped, SystemLoginObserved, SystemLogoutObserved, UserStopRequested,
};
use crate::module::status::{StatusRequest, StatusResponse};
use crate::module::upload::{FlushBatchNow, Upload};

pub struct IpcBridge {
    accept_rx: std::sync::mpsc::Receiver<RemoteEventBus>,
    clients: Arc<Mutex<Vec<RemoteSender>>>,
}

impl IpcBridge {
    /// Bind a Unix-domain socket at `path` and spawn a blocking accept thread.
    /// Returns `None` on bind failure (logs to stderr).
    pub fn bind(path: &Path) -> Option<Self> {
        let (accept_tx, accept_rx) = std::sync::mpsc::channel::<RemoteEventBus>();
        match IpcListener::bind(path) {
            Ok(listener) => {
                std::thread::spawn(move || {
                    loop {
                        match listener.blocking_accept() {
                            Ok(remote) => {
                                if accept_tx.send(remote).is_err() {
                                    break;
                                }
                            }
                            Err(e) => {
                                eprintln!("daemon: ipc accept error: {e}");
                                break;
                            }
                        }
                    }
                });
                Some(Self {
                    accept_rx,
                    clients: Arc::new(Mutex::new(Vec::new())),
                })
            }
            Err(e) => {
                eprintln!(
                    "daemon: failed to bind IPC listener at {}: {e}",
                    path.display()
                );
                None
            }
        }
    }

    /// Broadcast events of type `E` from the bus to all connected remote clients.
    /// Dead senders (disconnected peers, or whose write fails) are pruned.
    pub fn subscribe_outbound<E: Event + Clone>(&self, bus: &mut EventBus) {
        let c = self.clients.clone();
        bus.subscribe::<E>(move |ev| {
            c.lock()
                .unwrap()
                .retain(|s| s.is_connected() && s.send(ev.clone()).is_ok());
            Ok(())
        });
    }

    /// Subscribe the standard daemon→controller set:
    /// `LoginResult`, `LogoutResult`, `StatusResponse`, `Logout`, `EventError`.
    pub fn subscribe_standard_outbound(&self, bus: &mut EventBus) {
        self.subscribe_outbound::<LoginResult>(bus);
        self.subscribe_outbound::<LogoutResult>(bus);
        self.subscribe_outbound::<StatusResponse>(bus);
        self.subscribe_outbound::<Logout>(bus);
        self.subscribe_outbound::<EventError>(bus);
    }

    /// Register handlers on `remote` to forward the standard controller→daemon set
    /// into the bus via `emitter`:
    /// `LoginRequested`, `LogoutRequested`, `StatusRequest`, `UserStopRequested`,
    /// `SystemLoginObserved`, `SystemLogoutObserved`, `ProcessStopped`, `Upload`,
    /// `FlushBatchNow`.
    pub fn forward_standard_inbound(remote: &mut RemoteEventBus, emitter: &Emitter) {
        macro_rules! forward {
            ($($T:ty),* $(,)?) => {
                $(let e = emitter.clone(); remote.on::<$T>(move |ev| e.send(ev.clone()));)*
            };
        }
        forward!(
            LoginRequested,
            LogoutRequested,
            StatusRequest,
            UserStopRequested,
            SystemLoginObserved,
            SystemLogoutObserved,
            ProcessStopped,
            Upload,
            FlushBatchNow,
        );
    }

    /// Drain newly accepted connections, calling `setup` on each before storing
    /// its outbound sender. `setup` is `Fn` (not `FnOnce`) — called once per connection.
    pub fn accept_pending(
        &mut self,
        bus: &mut EventBus,
        setup: impl Fn(&mut RemoteEventBus, &Emitter),
    ) {
        // Drop senders for peers that have disconnected. This runs every daemon
        // loop iteration so dead connections are reclaimed even when no outbound
        // event is being broadcast, preventing the socket fds from leaking.
        self.clients.lock().unwrap().retain(|s| s.is_connected());

        while let Ok(mut remote) = self.accept_rx.try_recv() {
            let e = bus.emitter();
            setup(&mut remote, &e);
            self.clients.lock().unwrap().push(remote.sender());
            remote.start();
        }
    }
}
