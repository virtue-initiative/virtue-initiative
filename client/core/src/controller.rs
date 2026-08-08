use crate::error::{CoreError, CoreResult};
use crate::events::{Event, EventChannel};
use crate::model::Redacted;
use crate::model::ServiceStatus;
use crate::module::auth::{LoginRequested, LoginResult, LogoutRequested, LogoutResult};
use crate::module::lifecycle::UserStopRequested;
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
            Err(CoreError::Remote(
                r.error
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "login failed".to_string()),
            ))
        }
    }

    /// Send `LogoutRequested` and block until `LogoutResult` is received.
    pub fn logout(&mut self) -> CoreResult<()> {
        let r: LogoutResult = self.channel.request(LogoutRequested)?;
        if r.success {
            Ok(())
        } else {
            Err(CoreError::Remote(
                r.error
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "logout failed".to_string()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::bus::{Emitter, EventBus, Observer, StateType};
    use std::any::Any;

    /// Replies to `LoginRequested`/`LogoutRequested` with a failure result
    /// carrying an empty error string, mimicking a daemon-reported failure
    /// with no message (e.g. an empty response body upstream).
    struct EmptyErrorResponder;

    impl Observer for EmptyErrorResponder {
        fn init(&mut self, _bus: &mut EventBus, _state: StateType) -> CoreResult<()> {
            Ok(())
        }

        fn on_event(&mut self, event: &dyn Any, emitter: &Emitter) -> CoreResult<()> {
            crate::dispatch_event!(event, {
                _: LoginRequested => emitter.send(LoginResult {
                    success: false,
                    error: Some(String::new()),
                    device_id: None,
                }),
                _: LogoutRequested => emitter.send(LogoutResult {
                    success: false,
                    error: Some(String::new()),
                }),
            })
        }

        fn save(&self) -> CoreResult<StateType> {
            Ok(StateType::Null)
        }

        fn name(&self) -> &'static str {
            "empty_error_responder"
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    #[test]
    fn empty_login_error_becomes_default_remote_message() {
        let bus = EventBus::new(vec![Box::new(EmptyErrorResponder)], StateType::Null).unwrap();
        let mut controller = ClientController::new(bus);

        let err = controller
            .login("user@example.com", "password", None)
            .expect_err("login should fail");

        match err {
            CoreError::Remote(message) => assert_eq!(message, "login failed"),
            other => panic!("expected CoreError::Remote, got {other:?}"),
        }
    }

    #[test]
    fn empty_logout_error_becomes_default_remote_message() {
        let bus = EventBus::new(vec![Box::new(EmptyErrorResponder)], StateType::Null).unwrap();
        let mut controller = ClientController::new(bus);

        let err = controller.logout().expect_err("logout should fail");

        match err {
            CoreError::Remote(message) => assert_eq!(message, "logout failed"),
            other => panic!("expected CoreError::Remote, got {other:?}"),
        }
    }
}
