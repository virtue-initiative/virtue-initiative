use std::path::Path;
use std::sync::Arc;

use crate::api::ApiTransport;
use crate::daemon::Daemon;
use crate::events::remote::{IpcListener, RemoteEventBus};
use crate::module::auth::{LoginRequested, LoginResult, LogoutRequested, LogoutResult};
use crate::module::lifecycle::UserStopRequested;
use crate::module::status::{StatusRequest, StatusResponse};
use crate::module::upload::{FlushBatchNow, Upload};
use crate::platform::PlatformHooks;

/// Accepts Unix-socket connections and wires each one's inbound requests
/// directly to a shared [`Daemon`]'s synchronous methods, replying on that
/// same connection. Retains a small broadcast list (via
/// [`Daemon::add_broadcast_target`]) only for genuinely daemon-initiated
/// pushes (`Logout`) — everything else is request/response, handled inline
/// per connection.
pub struct IpcBridge {
    accept_rx: std::sync::mpsc::Receiver<RemoteEventBus>,
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
                                tracing::error!(error = %e, "daemon: ipc accept error");
                                break;
                            }
                        }
                    }
                });
                Some(Self { accept_rx })
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    path = %path.display(),
                    "daemon: failed to bind IPC listener"
                );
                None
            }
        }
    }

    /// Drain newly-accepted connections, wiring each one directly to
    /// `daemon` and starting its reader thread. Call this periodically (e.g.
    /// once per `Daemon::run_forever` wakeup) from the platform daemon loop.
    pub fn accept_pending<P, A>(&mut self, daemon: &Arc<Daemon<P, A>>)
    where
        P: PlatformHooks,
        A: ApiTransport + Send + Sync + 'static,
    {
        while let Ok(mut remote) = self.accept_rx.try_recv() {
            let sender = remote.sender();
            daemon.add_broadcast_target(sender.clone());

            let d = Arc::clone(daemon);
            let s = sender.clone();
            remote.subscribe(move |req: &LoginRequested| {
                let result = match d.login(&req.email, &req.password, req.device_name.as_deref()) {
                    Ok(device_id) => LoginResult {
                        success: true,
                        error: None,
                        device_id: Some(device_id),
                    },
                    Err(err) => LoginResult {
                        success: false,
                        error: Some(err.to_string()),
                        device_id: None,
                    },
                };
                s.send(result)
            });

            let d = Arc::clone(daemon);
            let s = sender.clone();
            remote.subscribe(move |_: &LogoutRequested| {
                let result = match d.logout() {
                    Ok(()) => LogoutResult {
                        success: true,
                        error: None,
                    },
                    Err(err) => LogoutResult {
                        success: false,
                        error: Some(err.to_string()),
                    },
                };
                s.send(result)
            });

            let d = Arc::clone(daemon);
            let s = sender.clone();
            remote
                .subscribe(move |_: &StatusRequest| s.send(StatusResponse { status: d.status() }));

            let d = Arc::clone(daemon);
            remote.subscribe(move |req: &UserStopRequested| {
                d.note_user_stop(&req.source);
                Ok(())
            });

            let d = Arc::clone(daemon);
            remote.subscribe(move |req: &Upload| {
                d.queue_upload(req.clone());
                Ok(())
            });

            let d = Arc::clone(daemon);
            remote.subscribe(move |_: &FlushBatchNow| {
                d.flush_batch_now();
                Ok(())
            });

            remote.start();
        }
    }
}
