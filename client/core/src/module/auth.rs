use std::any::Any;
use std::sync::mpsc::Sender;

use serde::{Deserialize, Serialize};

use crate::api::ApiTransport;
use crate::error::{CoreError, CoreResult};
use crate::events::{Event, Observer, PartialStatus, StateType, log_error};
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

pub struct AuthObserver<A: ApiTransport, C = ()> {
    pub(crate) state: AuthObserverState,
    pub(crate) api: A,
    device_name: String,
    platform_name: String,
    tx: Sender<Event<C>>,
    needs_settings_refresh: bool,
    pings_without_refresh: u32,
}

impl<A: ApiTransport, C: 'static> AuthObserver<A, C> {
    pub fn new(api: A, device_name: String, platform_name: String, tx: Sender<Event<C>>) -> Self {
        Self {
            state: AuthObserverState::default(),
            api,
            device_name,
            platform_name,
            tx,
            needs_settings_refresh: false,
            pings_without_refresh: 0,
        }
    }

    fn handle_login_requested(&mut self, email: &str, password: &str) {
        let result = self.do_login(email, password);
        match result {
            Ok((credentials, settings)) => {
                let device_id = credentials.device_id.clone();
                self.tx
                    .send(Event::Login {
                        credentials,
                        settings,
                    })
                    .ok();
                self.tx
                    .send(Event::LoginResult {
                        success: true,
                        error: None,
                        device_id: Some(device_id),
                    })
                    .ok();
            }
            Err(e) => {
                self.tx
                    .send(Event::LoginResult {
                        success: false,
                        error: Some(e.to_string()),
                        device_id: None,
                    })
                    .ok();
            }
        }
    }

    fn do_login(
        &mut self,
        email: &str,
        password: &str,
    ) -> CoreResult<(
        crate::model::DeviceCredentials,
        crate::model::DeviceSettings,
    )> {
        let access_token = self.api.login(email, password)?;
        let mut device =
            self.api
                .register_device(&access_token, &self.device_name, &self.platform_name)?;

        let api = &self.api;
        let settings = with_device_token_retry(api, &mut device, |api, token| {
            api.get_device_settings(token)
        })?;

        self.state.user_access_token = Some(access_token);
        self.state.device_credentials = Some(device.clone());
        self.needs_settings_refresh = false;
        self.pings_without_refresh = 0;
        Ok((device, settings))
    }

    fn handle_logout_requested(&mut self) {
        if let Some(token) = self.state.user_access_token.clone() {
            let _ = self.api.logout(&token);
        }

        self.state.user_access_token = None;
        self.state.device_credentials = None;
        self.needs_settings_refresh = false;
        self.pings_without_refresh = 0;

        self.tx.send(Event::Logout).ok();
        self.tx
            .send(Event::LogoutResult {
                success: true,
                error: None,
            })
            .ok();
    }

    fn handle_ping(&mut self) {
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
        match self.refresh_settings() {
            Ok(settings) => {
                self.needs_settings_refresh = false;
                self.tx
                    .send(Event::DeviceSettingsRefreshed { settings })
                    .ok();
            }
            Err(e) => {
                log_error("settings refresh failed on ping", Some(&e));
            }
        }
    }

    fn refresh_settings(&mut self) -> CoreResult<crate::model::DeviceSettings> {
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
                self.tx.send(Event::Logout).ok();
                Err(CoreError::NotAuthenticated)
            }
            Err(err) => Err(err),
        }
    }
}

impl<C: 'static, A: ApiTransport + 'static> Observer<C> for AuthObserver<A, C> {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn name(&self) -> &'static str {
        "auth"
    }

    fn save_state(&self) -> CoreResult<StateType> {
        Ok(serde_json::to_value(&self.state)?)
    }

    fn load_state(&mut self, state: StateType) -> CoreResult<()> {
        // Null was written by a prior version that didn't persist auth here.
        if !state.is_null() {
            self.state = serde_json::from_value(state)?;
        }
        self.needs_settings_refresh = self.state.device_credentials.is_some();
        Ok(())
    }

    fn on_event(&mut self, event: &Event<C>) -> CoreResult<()> {
        match event {
            Event::LoginRequested { email, password } => {
                self.handle_login_requested(email, password);
            }
            Event::LogoutRequested => {
                self.handle_logout_requested();
            }
            Event::Ping => {
                self.handle_ping();
            }
            Event::StatusRequest => {
                let device_id = self
                    .state
                    .device_credentials
                    .as_ref()
                    .map(|c| c.device_id.clone());
                self.tx
                    .send(Event::PartialStatus(PartialStatus::Auth {
                        is_authenticated: self.state.device_credentials.is_some(),
                        device_id,
                    }))
                    .ok();
            }
            _ => {}
        }
        Ok(())
    }
}
