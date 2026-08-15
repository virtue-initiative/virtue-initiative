use std::any::Any;

use serde::{Deserialize, Serialize};

use crate::api::ApiTransport;
use crate::error::CoreResult;
use crate::events::bus::{Emitter, EventBus, Observer, StateType};
use crate::model::PartialStatus;
use crate::model::{DeviceCredentials, DeviceSettings, Redacted};
use crate::module::status::StatusRequest;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Login {
    pub credentials: DeviceCredentials,
    pub settings: DeviceSettings,
    pub hash_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Logout;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginRequested {
    pub email: String,
    pub password: Redacted<String>,
    /// Optional device-name override chosen by the user at login. When `Some`
    /// and non-empty, it takes precedence over the construction-time
    /// `AuthModule::device_name` (hostname / OS device name).
    #[serde(default)]
    pub device_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginResult {
    pub success: bool,
    pub error: Option<String>,
    pub device_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogoutRequested;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogoutResult {
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct AuthObserverState {
    pub device_credentials: Option<DeviceCredentials>,
}

pub struct AuthModule<A: ApiTransport + Send + Sync + 'static> {
    pub state: AuthObserverState,
    api: A,
    device_name: String,
    platform_name: String,
}

impl<A: ApiTransport + Send + Sync + 'static> AuthModule<A> {
    pub fn new(api: A, device_name: String, platform_name: String) -> Self {
        Self {
            state: AuthObserverState::default(),
            api,
            device_name,
            platform_name,
        }
    }

    fn handle_login_requested(
        &mut self,
        email: &str,
        password: &str,
        device_name: Option<&str>,
        emitter: &Emitter,
    ) {
        // A login while another device session is still active (e.g. the
        // user re-runs `login` without logging out first) would otherwise
        // leave the old device row active on the server forever. Log it out
        // first so it gets soft-deleted and its hash state reset, same as an
        // explicit logout.
        if self.revoke_current_device() {
            let _ = emitter.send(Logout);
        }
        match self.do_login(email, password, device_name) {
            Ok((credentials, settings, hash_token)) => {
                let device_id = credentials.device_id.clone();
                let _ = emitter.send(Login {
                    credentials,
                    settings,
                    hash_token,
                });
                let _ = emitter.send(LoginResult {
                    success: true,
                    error: None,
                    device_id: Some(device_id),
                });
            }
            Err(e) => {
                let _ = emitter.send(LoginResult {
                    success: false,
                    error: Some(e.to_string()),
                    device_id: None,
                });
            }
        }
    }

    fn do_login(
        &mut self,
        email: &str,
        password: &str,
        device_name: Option<&str>,
    ) -> CoreResult<(DeviceCredentials, crate::model::DeviceSettings, String)> {
        // Use the user-supplied override when present and non-empty (trimmed),
        // otherwise fall back to the construction-time device name (hostname).
        let resolved_name = device_name
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(self.device_name.as_str());
        let registered =
            self.api
                .register_device(email, password, resolved_name, &self.platform_name)?;
        self.state.device_credentials = Some(registered.credentials.clone());
        Ok((
            registered.credentials,
            registered.settings,
            registered.hash_token,
        ))
    }

    fn handle_logout_requested(&mut self, emitter: &Emitter) {
        self.revoke_current_device();
        let _ = emitter.send(Logout);
        let _ = emitter.send(LogoutResult {
            success: true,
            error: None,
        });
    }

    /// Revokes the current device's session with the server (best effort)
    /// and clears local credentials. Returns `true` if there was a device
    /// session to revoke.
    fn revoke_current_device(&mut self) -> bool {
        if let Some(creds) = self.state.device_credentials.take() {
            let _ = self.api.logout(&creds.refresh_token);
            true
        } else {
            false
        }
    }
}

impl<A: ApiTransport + Send + Sync + 'static> Observer for AuthModule<A> {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn name(&self) -> &'static str {
        "auth"
    }

    fn init(&mut self, _bus: &mut EventBus, state: StateType) -> CoreResult<()> {
        if !state.is_null() {
            self.state = serde_json::from_value(state)?;
        }
        Ok(())
    }

    fn on_event(&mut self, event: &dyn Any, emitter: &Emitter) -> CoreResult<()> {
        crate::dispatch_event!(event, {
            ev: LoginRequested => {
                self.handle_login_requested(
                    &ev.email,
                    &ev.password,
                    ev.device_name.as_deref(),
                    emitter,
                );
                Ok(())
            },
            _: LogoutRequested => {
                self.handle_logout_requested(emitter);
                Ok(())
            },
            _: StatusRequest => {
                let device_id = self.state.device_credentials
                    .as_ref()
                    .map(|c| c.device_id.clone());
                let _ = emitter.send(PartialStatus::Auth {
                    is_authenticated: self.state.device_credentials.is_some(),
                    device_id,
                });
                Ok(())
            },
        })
    }

    fn save(&self) -> CoreResult<StateType> {
        Ok(serde_json::to_value(&self.state)?)
    }
}

#[cfg(test)]
mod tests {
    use super::{AuthModule, Login, LoginRequested, LoginResult, Logout, LogoutRequested};
    use crate::error::CoreError;
    use crate::model::{PartialStatus, Redacted};
    use crate::module::status::StatusRequest;
    use crate::testing::EventTester;

    #[test]
    fn login_success_emits_login_and_result() {
        let mut b = EventTester::builder();
        b.capture::<Login>().capture::<LoginResult>();
        b.add(AuthModule::new(
            b.api(),
            "test-device".into(),
            "test-platform".into(),
        ));
        let mut t = b.build();
        t.emit(
            1,
            LoginRequested {
                email: "alice@example.org".into(),
                password: Redacted("secret".into()),
                device_name: None,
            },
        );
        assert_eq!(t.captured::<Login>().len(), 1, "expected Login event");
        let results = t.captured::<LoginResult>();
        assert_eq!(results.len(), 1, "expected LoginResult event");
        assert!(results[0].success, "login result should be success");
        assert!(
            results[0].device_id.is_some(),
            "login result should carry device_id"
        );
    }

    #[test]
    fn login_uses_device_name_override_when_present() {
        let mut b = EventTester::builder();
        b.capture::<LoginResult>();
        b.add(AuthModule::new(
            b.api(),
            "test-device".into(),
            "test-platform".into(),
        ));
        let mut t = b.build();
        t.emit(
            1,
            LoginRequested {
                email: "alice@example.org".into(),
                password: Redacted("secret".into()),
                device_name: Some("  My Laptop  ".into()),
            },
        );
        let calls = t.api.state().register_device_calls.clone();
        assert_eq!(calls.len(), 1, "expected one register_device call");
        assert_eq!(
            calls[0].name, "My Laptop",
            "override name should be used (trimmed)"
        );
    }

    #[test]
    fn login_falls_back_to_construction_name_when_override_blank() {
        let mut b = EventTester::builder();
        b.capture::<LoginResult>();
        b.add(AuthModule::new(
            b.api(),
            "test-device".into(),
            "test-platform".into(),
        ));
        let mut t = b.build();
        t.emit(
            1,
            LoginRequested {
                email: "alice@example.org".into(),
                password: Redacted("secret".into()),
                device_name: Some("   ".into()),
            },
        );
        let calls = t.api.state().register_device_calls.clone();
        assert_eq!(calls.len(), 1, "expected one register_device call");
        assert_eq!(
            calls[0].name, "test-device",
            "blank override should fall back to construction-time name"
        );
    }

    #[test]
    fn login_failure_emits_failed_result() {
        let mut b = EventTester::builder();
        b.capture::<LoginResult>();
        b.add(AuthModule::new(
            b.api(),
            "test-device".into(),
            "test-platform".into(),
        ));
        let mut t = b.build();
        t.api
            .program_register_device(Err(CoreError::InvalidState("bad credentials")));
        t.emit(
            1,
            LoginRequested {
                email: "alice@example.org".into(),
                password: Redacted("wrong".into()),
                device_name: None,
            },
        );
        let results = t.captured::<LoginResult>();
        assert_eq!(results.len(), 1);
        assert!(!results[0].success, "login result should be failure");
        assert!(
            results[0].error.is_some(),
            "failed login should carry error message"
        );
    }

    #[test]
    fn login_without_prior_session_does_not_call_logout() {
        let mut b = EventTester::builder();
        b.add(AuthModule::new(
            b.api(),
            "test-device".into(),
            "test-platform".into(),
        ));
        let mut t = b.build();
        t.emit(
            1,
            LoginRequested {
                email: "alice@example.org".into(),
                password: Redacted("secret".into()),
                device_name: None,
            },
        );
        assert!(
            t.api.state().logout_calls.is_empty(),
            "first login should not call logout when there is no existing session"
        );
    }

    #[test]
    fn login_while_already_authenticated_logs_out_previous_device_first() {
        let mut b = EventTester::builder();
        b.capture::<Logout>().capture::<Login>();
        b.add(AuthModule::new(
            b.api(),
            "test-device".into(),
            "test-platform".into(),
        ));
        let mut t = b.build();
        t.emit(
            1,
            LoginRequested {
                email: "alice@example.org".into(),
                password: Redacted("secret".into()),
                device_name: None,
            },
        );
        t.emit(
            2,
            LoginRequested {
                email: "bob@example.org".into(),
                password: Redacted("secret2".into()),
                device_name: None,
            },
        );
        let calls = t.api.state().logout_calls.clone();
        assert_eq!(
            calls.len(),
            1,
            "second login should log out the previous device's session"
        );
        assert_eq!(
            calls[0], "mock-refresh-token",
            "logout should use the previous device's refresh token"
        );
        assert_eq!(
            t.captured::<Logout>().len(),
            1,
            "expected exactly one implicit Logout, for the second login"
        );
        assert_eq!(
            t.captured::<Login>().len(),
            2,
            "expected both logins to emit Login"
        );
    }

    #[test]
    fn logout_requested_emits_logout_and_result() {
        let mut b = EventTester::builder();
        b.capture::<Logout>();
        b.add(AuthModule::new(
            b.api(),
            "test-device".into(),
            "test-platform".into(),
        ));
        let mut t = b.build();
        t.emit(1, LogoutRequested);
        assert_eq!(t.captured::<Logout>().len(), 1, "expected Logout event");
    }

    #[test]
    fn logout_without_credentials_does_not_call_api() {
        let mut b = EventTester::builder();
        b.add(AuthModule::new(
            b.api(),
            "test-device".into(),
            "test-platform".into(),
        ));
        let mut t = b.build();
        t.emit(1, LogoutRequested);
        assert!(
            t.api.state().logout_calls.is_empty(),
            "logout should not call the API when there are no device credentials"
        );
    }

    #[test]
    fn logout_with_credentials_calls_api_with_device_refresh_token() {
        let mut b = EventTester::builder();
        b.add(AuthModule::new(
            b.api(),
            "test-device".into(),
            "test-platform".into(),
        ));
        let mut t = b.build();
        t.emit(
            1,
            LoginRequested {
                email: "alice@example.org".into(),
                password: Redacted("secret".into()),
                device_name: None,
            },
        );
        t.emit(2, LogoutRequested);
        let calls = t.api.state().logout_calls.clone();
        assert_eq!(calls.len(), 1, "expected one logout call");
        assert_eq!(
            calls[0], "mock-refresh-token",
            "logout should be called with the device's refresh token"
        );
    }

    #[test]
    fn status_request_emits_auth_partial_status() {
        let mut b = EventTester::builder();
        b.add(AuthModule::new(
            b.api(),
            "test-device".into(),
            "test-platform".into(),
        ));
        let mut t = b.build();
        t.emit(1, StatusRequest);
        t.assert_like::<PartialStatus>(crate::like!(PartialStatus::Auth {
            is_authenticated: false,
            ..
        }));
    }
}
