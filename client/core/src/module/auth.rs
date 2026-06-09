use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::api::ApiTransport;
use crate::error::{CoreError, CoreResult};
use crate::events::bus::{Emitter, EventBus, Observer, StateType, log_error};
use crate::events::types::{
    ConfigChanged, DeviceSettingsRefreshed, Login, LoginRequested, LoginResult, Logout,
    LogoutRequested, LogoutResult, PartialStatus, Ping, StatusRequest,
};
use crate::model::DeviceCredentials;

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

pub(crate) struct AuthInner<A: ApiTransport> {
    pub(crate) state: AuthObserverState,
    api: A,
    device_name: String,
    platform_name: String,
    needs_settings_refresh: bool,
    pings_without_refresh: u32,
}

impl<A: ApiTransport> AuthInner<A> {
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

pub struct AuthModule<A: ApiTransport + Send + Sync + 'static> {
    pub(crate) inner: Arc<Mutex<AuthInner<A>>>,
}

impl<A: ApiTransport + Send + Sync + 'static> AuthModule<A> {
    pub fn new(api: A, device_name: String, platform_name: String) -> Self {
        Self {
            inner: Arc::new(Mutex::new(AuthInner {
                state: AuthObserverState::default(),
                api,
                device_name,
                platform_name,
                needs_settings_refresh: false,
                pings_without_refresh: 0,
            })),
        }
    }
}

impl<A: ApiTransport + Send + Sync + 'static> Observer for AuthModule<A> {
    fn name(&self) -> &'static str {
        "auth"
    }

    fn init(&mut self, bus: &mut EventBus, state: StateType) -> CoreResult<()> {
        {
            let mut g = self.inner.lock().unwrap();
            // Null was written by a prior version that didn't persist auth here.
            if !state.is_null() {
                g.state = serde_json::from_value(state)?;
            }
            g.needs_settings_refresh = g.state.device_credentials.is_some();
        }

        let emitter = bus.emitter();

        let inner = Arc::clone(&self.inner);
        let e = emitter.clone();
        bus.subscribe(move |ev: &LoginRequested| {
            inner
                .lock()
                .unwrap()
                .handle_login_requested(&ev.email, &ev.password, &e);
            Ok(())
        });

        let inner = Arc::clone(&self.inner);
        let e = emitter.clone();
        bus.subscribe(move |_: &LogoutRequested| {
            inner.lock().unwrap().handle_logout_requested(&e);
            Ok(())
        });

        let inner = Arc::clone(&self.inner);
        let e = emitter.clone();
        bus.subscribe(move |_: &Ping| {
            inner.lock().unwrap().handle_ping(&e);
            Ok(())
        });

        let inner = Arc::clone(&self.inner);
        let e = emitter.clone();
        bus.subscribe(move |_: &StatusRequest| {
            let g = inner.lock().unwrap();
            let device_id = g
                .state
                .device_credentials
                .as_ref()
                .map(|c| c.device_id.clone());
            let _ = e.send(PartialStatus::Auth {
                is_authenticated: g.state.device_credentials.is_some(),
                device_id,
            });
            Ok(())
        });

        let inner = Arc::clone(&self.inner);
        bus.subscribe(move |ev: &ConfigChanged| {
            inner.lock().unwrap().api.reconfigure(&ev.api_base_url)
        });

        Ok(())
    }

    fn save(&self) -> CoreResult<StateType> {
        Ok(serde_json::to_value(&self.inner.lock().unwrap().state)?)
    }
}
