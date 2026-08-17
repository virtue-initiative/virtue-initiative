use std::sync::mpsc;

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::error::{CoreError, CoreResult};

/// Anything that can travel over an [`EventChannel`] (currently only
/// [`RemoteEventBus`](super::RemoteEventBus), the cross-process IPC
/// channel — the in-process `EventBus` this trait originally also served has
/// been retired in favor of `Daemon`'s direct method calls).
pub trait Event: Serialize + DeserializeOwned + std::fmt::Debug + Send + Sync + 'static {}

impl<T> Event for T where T: Serialize + DeserializeOwned + std::fmt::Debug + Send + Sync + 'static {}

/// Emitted by [`RemoteEventBus`](super::RemoteEventBus) senders to report a
/// failure to a connected controller/UI (e.g. `Daemon`-side errors it wants
/// to surface).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Error {
    pub source: String,
    pub message: String,
}

/// A channel on which typed events can be published and observed.
///
/// Implemented by [`RemoteEventBus`](super::RemoteEventBus) (cross-process).
/// `ClientController` is written once against `EventChannel` so it works
/// regardless of the transport underneath.
pub trait EventChannel {
    /// Publish `event` to the channel; does not wait for a reply.
    fn publish<E: Event>(&self, event: E) -> CoreResult<()>;

    /// Register `handler` to run for every observed event of type `E`.
    fn on<E: Event>(&mut self, handler: impl Fn(&E) -> CoreResult<()> + Send + Sync + 'static);

    /// Drive any pending in-process work. A no-op for `RemoteEventBus`, whose
    /// reader thread does the work; kept for interface symmetry.
    fn pump(&mut self) -> CoreResult<()>;

    /// Publish `request` and block until a matching `Resp` is observed, or 5
    /// seconds elapse.
    fn request<Req: Event, Resp: Event + Clone>(&mut self, request: Req) -> CoreResult<Resp> {
        let (tx, rx) = mpsc::channel();
        self.on::<Resp>(move |resp| {
            let _ = tx.send(resp.clone());
            Ok(())
        });
        self.publish(request)?;
        self.pump()?;
        rx.recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| match e {
                mpsc::RecvTimeoutError::Timeout => {
                    CoreError::CommandFailed("timed out waiting for daemon response".into())
                }
                mpsc::RecvTimeoutError::Disconnected => {
                    CoreError::InvalidState("event channel closed before response")
                }
            })
    }
}
