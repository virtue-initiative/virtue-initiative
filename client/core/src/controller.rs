use crate::error::{CoreError, CoreResult};
use crate::events::{Event, EventChannel};
use crate::model::ServiceStatus;
use crate::model::{ProcessStoppedReason, Redacted};
use crate::module::auth::{LoginRequested, LoginResult, LogoutRequested, LogoutResult};
use crate::module::lifecycle::{ProcessStopped, UserStopRequested};
use crate::module::status::{StatusRequest, StatusResponse};
use crate::module::upload::{FlushBatchNow, Upload};

/// High-level client for communicating with a daemon over any [`EventChannel`].
///
/// Generic over `C` so the same implementation works whether `C` is an
/// in-process [`EventBus`] (tests, fully in-process use) or a
/// [`RemoteEventBus`] (Linux/macOS socket, Windows in-process channel).
///
/// [`EventBus`]: crate::events::EventBus
/// [`RemoteEventBus`]: crate::events::RemoteEventBus
pub struct ClientController<C: EventChannel> {
    channel: C,
}

impl<C: EventChannel> ClientController<C> {
    pub fn new(channel: C) -> Self {
        Self { channel }
    }

    /// Send `LoginRequested` and block until `LoginResult` is received.
    /// Returns the device ID on success.
    pub fn login(
        &mut self,
        email: &str,
        password: &str,
        device_name: Option<&str>,
    ) -> CoreResult<String> {
        let r: LoginResult = self.channel.request(LoginRequested {
            email: email.into(),
            password: Redacted(password.into()),
            device_name: device_name.map(|name| name.into()),
        })?;
        if r.success {
            Ok(r.device_id.unwrap_or_default())
        } else {
            Err(CoreError::CommandFailed(
                r.error.unwrap_or_else(|| "login failed".to_string()),
            ))
        }
    }

    /// Send `LogoutRequested` and block until `LogoutResult` is received.
    pub fn logout(&mut self) -> CoreResult<()> {
        let r: LogoutResult = self.channel.request(LogoutRequested)?;
        if r.success {
            Ok(())
        } else {
            Err(CoreError::CommandFailed(
                r.error.unwrap_or_else(|| "logout failed".to_string()),
            ))
        }
    }

    /// Send `StatusRequest` and block until `StatusResponse` is received.
    pub fn get_status(&mut self) -> CoreResult<ServiceStatus> {
        let r: StatusResponse = self.channel.request(StatusRequest)?;
        Ok(r.status)
    }

    pub fn request_user_stop(&self, source: &str) -> CoreResult<()> {
        self.channel.publish(UserStopRequested {
            source: source.into(),
        })
    }

    pub fn note_process_stopped(&self, reason: ProcessStoppedReason) -> CoreResult<()> {
        self.channel.publish(ProcessStopped(reason))
    }

    /// Queue `upload` into the daemon's live batch/hash pipeline. Picked up on
    /// the daemon's next ping cycle (≤1s), same as an in-process `Upload`.
    pub fn queue_upload(&self, upload: Upload) -> CoreResult<()> {
        self.channel.publish(upload)
    }

    /// Ask the daemon to flush its currently queued batch items now, instead
    /// of waiting for the batch interval timer.
    pub fn flush_batch_now(&self) -> CoreResult<()> {
        self.channel.publish(FlushBatchNow)
    }

    /// Register a handler for events the daemon pushes unprompted.
    pub fn on<E: Event>(&mut self, handler: impl Fn(&E) -> CoreResult<()> + Send + Sync + 'static) {
        self.channel.on(handler)
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl ClientController<crate::events::RemoteEventBus> {
    /// Connect to the daemon at `path` and return a controller backed by a
    /// [`RemoteEventBus`].
    ///
    /// [`RemoteEventBus`]: crate::events::RemoteEventBus
    pub fn connect(path: &std::path::Path) -> CoreResult<Self> {
        let bus = crate::events::RemoteEventBus::connect(path).map_err(CoreError::from)?;
        Ok(Self::new(bus))
    }
}
