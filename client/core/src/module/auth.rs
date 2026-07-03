use std::any::Any;

use serde::{Deserialize, Serialize};

use crate::api::ApiTransport;
use crate::error::CoreResult;
use crate::events::bus::{Emitter, EventBus, Observer, StateType};
use crate::model::PartialStatus;
use crate::model::{DeviceCredentials, DeviceSettings, Redacted};
use crate::module::config::ConfigChanged;
use crate::module::status::StatusRequest;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Login {
    pub credentials: DeviceCredentials,
    pub settings: DeviceSettings,
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
        match self.do_login(email, password, device_name) {
            Ok((credentials, settings)) => {
                let device_id = credentials.device_id.clone();
                let _ = emitter.send(Login {
                    credentials,
                    settings,
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
    ) -> CoreResult<(DeviceCredentials, crate::model::DeviceSettings)> {
        let user_token = self.api.login(email, password)?;
        // Use the user-supplied override when present and non-empty (trimmed),
        // otherwise fall back to the construction-time device name (hostname).
        let resolved_name = device_name
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or(self.device_name.as_str());
        let device = self
            .api
            .register_device(&user_token, resolved_name, &self.platform_name)?;
        let settings = self.api.get_device_settings(&device.refresh_token)?;
        self.state.device_credentials = Some(device.clone());
        Ok((device, settings))
    }

    fn handle_logout_requested(&mut self, emitter: &Emitter) {
        let _ = self.api.logout();
        self.state.device_credentials = None;
        let _ = emitter.send(Logout);
        let _ = emitter.send(LogoutResult {
            success: true,
            error: None,
        });
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
            ev: ConfigChanged => self.api.reconfigure(&ev.api_base_url),
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
            .program_login(Err(CoreError::InvalidState("bad credentials")));
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
