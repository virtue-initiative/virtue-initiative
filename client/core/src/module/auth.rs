use std::any::Any;

use serde::{Deserialize, Serialize};

use crate::api::ApiTransport;
use crate::error::{CoreError, CoreResult};
use crate::events::Ping;
use crate::events::bus::{Emitter, EventBus, Observer, StateType, log_error};
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSettingsRefreshed {
    pub settings: DeviceSettings,
}

const SETTINGS_REFRESH_INTERVAL_PINGS: u32 = 3600;

fn with_device_token_retry<A: ApiTransport, T>(
    api: &A,
    credentials: &mut DeviceCredentials,
    mut op: impl FnMut(&A, &str) -> CoreResult<T>,
) -> CoreResult<T> {
    match op(api, &credentials.access_token) {
        Err(e) if e.is_unauthorized() => {
            let refreshed = api.refresh_device_token(&credentials.refresh_token)?;
            credentials.access_token = refreshed.clone();
            op(api, &refreshed)
        }
        other => other,
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct AuthObserverState {
    pub user_access_token: Option<String>,
    pub device_credentials: Option<DeviceCredentials>,
}

pub struct AuthModule<A: ApiTransport + Send + Sync + 'static> {
    pub state: AuthObserverState,
    api: A,
    device_name: String,
    platform_name: String,
    needs_settings_refresh: bool,
    pings_without_refresh: u32,
}

impl<A: ApiTransport + Send + Sync + 'static> AuthModule<A> {
    pub fn new(api: A, device_name: String, platform_name: String) -> Self {
        Self {
            state: AuthObserverState::default(),
            api,
            device_name,
            platform_name,
            needs_settings_refresh: false,
            pings_without_refresh: 0,
        }
    }

    fn handle_login_requested(&mut self, email: &str, password: &str, emitter: &Emitter) {
        match self.do_login(email, password) {
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
    ) -> CoreResult<(DeviceCredentials, crate::model::DeviceSettings)> {
        let access_token = self.api.login(email, password)?;
        let mut device =
            self.api
                .register_device(&access_token, &self.device_name, &self.platform_name)?;
        let settings = with_device_token_retry(&self.api, &mut device, |api, token| {
            api.get_device_settings(token)
        })?;
        self.state.user_access_token = Some(access_token);
        self.state.device_credentials = Some(device.clone());
        self.needs_settings_refresh = false;
        self.pings_without_refresh = 0;
        Ok((device, settings))
    }

    fn handle_logout_requested(&mut self, emitter: &Emitter) {
        if let Some(token) = self.state.user_access_token.clone() {
            let _ = self.api.logout(&token);
        }
        self.state.user_access_token = None;
        self.state.device_credentials = None;
        self.needs_settings_refresh = false;
        self.pings_without_refresh = 0;
        let _ = emitter.send(Logout);
        let _ = emitter.send(LogoutResult {
            success: true,
            error: None,
        });
    }

    fn handle_ping(&mut self, emitter: &Emitter) {
        if self.state.device_credentials.is_none() {
            return;
        }
        self.pings_without_refresh += 1;
        if self.pings_without_refresh >= SETTINGS_REFRESH_INTERVAL_PINGS {
            self.needs_settings_refresh = true;
            self.pings_without_refresh = 0;
        }
        if !self.needs_settings_refresh {
            return;
        }
        match self.refresh_settings(emitter) {
            Ok(settings) => {
                self.needs_settings_refresh = false;
                let _ = emitter.send(DeviceSettingsRefreshed { settings });
            }
            Err(e) => {
                log_error("settings refresh failed on ping", Some(&e));
            }
        }
    }

    fn refresh_settings(&mut self, emitter: &Emitter) -> CoreResult<crate::model::DeviceSettings> {
        let mut credentials = self
            .state
            .device_credentials
            .clone()
            .ok_or(CoreError::NotAuthenticated)?;
        let result = with_device_token_retry(&self.api, &mut credentials, |api, token| {
            api.get_device_settings(token)
        });
        match result {
            Ok(settings) => {
                self.state.device_credentials = Some(credentials);
                Ok(settings)
            }
            Err(err) if err.is_not_found() => {
                log_error("device not found; clearing local auth", Some(&err));
                self.state.user_access_token = None;
                self.state.device_credentials = None;
                let _ = emitter.send(Logout);
                Err(CoreError::NotAuthenticated)
            }
            Err(err) => Err(err),
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
        self.needs_settings_refresh = self.state.device_credentials.is_some();
        Ok(())
    }

    fn on_event(&mut self, event: &dyn Any, emitter: &Emitter) -> CoreResult<()> {
        crate::dispatch_event!(event, {
            ev: LoginRequested => {
                self.handle_login_requested(&ev.email, &ev.password, emitter);
                Ok(())
            },
            _: LogoutRequested => {
                self.handle_logout_requested(emitter);
                Ok(())
            },
            _: Ping => {
                self.handle_ping(emitter);
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
    use std::sync::{Arc, Mutex};

    use super::{AuthModule, LoginRequested, LoginResult, LogoutRequested};
    use super::{Login, Logout};
    use crate::error::CoreError;
    use crate::events::bus::{EventBus, StateType};
    use crate::model::PartialStatus;
    use crate::model::Redacted;
    use crate::module::status::StatusRequest;
    use crate::testing::MockApiClient;

    fn make() -> (EventBus, MockApiClient) {
        let api = MockApiClient::new();
        let module = AuthModule::new(api.clone(), "test-device".into(), "test-platform".into());
        let bus = EventBus::new(vec![Box::new(module)], StateType::Null).unwrap();
        (bus, api)
    }

    #[test]
    fn login_success_emits_login_and_result() {
        let (mut bus, _api) = make();
        let logins: Arc<Mutex<Vec<Login>>> = Arc::new(Mutex::new(Vec::new()));
        let results: Arc<Mutex<Vec<LoginResult>>> = Arc::new(Mutex::new(Vec::new()));
        let l = Arc::clone(&logins);
        let r = Arc::clone(&results);
        bus.subscribe(move |ev: &Login| {
            l.lock().unwrap().push(ev.clone());
            Ok(())
        });
        bus.subscribe(move |ev: &LoginResult| {
            r.lock().unwrap().push(ev.clone());
            Ok(())
        });

        bus.send(LoginRequested {
            email: "alice@example.org".into(),
            password: Redacted("secret".into()),
        })
        .unwrap();
        bus.iter().unwrap();

        assert_eq!(logins.lock().unwrap().len(), 1, "expected Login event");
        let results = results.lock().unwrap();
        assert_eq!(results.len(), 1, "expected LoginResult event");
        assert!(results[0].success, "login result should be success");
        assert!(
            results[0].device_id.is_some(),
            "login result should carry device_id"
        );
    }

    #[test]
    fn login_failure_emits_failed_result() {
        let (mut bus, api) = make();
        api.program_login(Err(CoreError::InvalidState("bad credentials")));

        let results: Arc<Mutex<Vec<LoginResult>>> = Arc::new(Mutex::new(Vec::new()));
        let r = Arc::clone(&results);
        bus.subscribe(move |ev: &LoginResult| {
            r.lock().unwrap().push(ev.clone());
            Ok(())
        });

        bus.send(LoginRequested {
            email: "alice@example.org".into(),
            password: Redacted("wrong".into()),
        })
        .unwrap();
        bus.iter().unwrap();

        let results = results.lock().unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].success, "login result should be failure");
        assert!(
            results[0].error.is_some(),
            "failed login should carry error message"
        );
    }

    #[test]
    fn logout_requested_emits_logout_and_result() {
        let (mut bus, _api) = make();
        let logouts: Arc<Mutex<Vec<Logout>>> = Arc::new(Mutex::new(Vec::new()));
        let l = Arc::clone(&logouts);
        bus.subscribe(move |ev: &Logout| {
            l.lock().unwrap().push(ev.clone());
            Ok(())
        });

        bus.send(LogoutRequested).unwrap();
        bus.iter().unwrap();

        assert_eq!(logouts.lock().unwrap().len(), 1, "expected Logout event");
    }

    #[test]
    fn status_request_emits_auth_partial_status() {
        let (mut bus, _api) = make();
        let partials: Arc<Mutex<Vec<PartialStatus>>> = Arc::new(Mutex::new(Vec::new()));
        let p = Arc::clone(&partials);
        bus.subscribe(move |ev: &PartialStatus| {
            p.lock().unwrap().push(ev.clone());
            Ok(())
        });

        bus.send(StatusRequest).unwrap();
        bus.iter().unwrap();

        let p = partials.lock().unwrap();
        assert!(
            p.iter().any(|s| matches!(
                s,
                PartialStatus::Auth {
                    is_authenticated: false,
                    ..
                }
            )),
            "expected unauthenticated PartialStatus::Auth"
        );
    }
}
