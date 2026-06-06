use std::any::Any;
use std::sync::mpsc::Sender;

use serde::{Deserialize, Serialize};

use crate::api::ApiTransport;
use crate::error::CoreResult;
use crate::events::{Event, Observer, PartialStatus, StateType, log_error};
use crate::model::DeviceCredentials;

#[derive(Serialize, Deserialize, Default)]
pub struct AuthObserverState {
    pub user_access_token: Option<String>,
    pub device_credentials: Option<DeviceCredentials>,
}

pub struct AuthObserver<A: ApiTransport> {
    pub(crate) state: AuthObserverState,
    pub(crate) api: A,
    device_name: String,
    platform_name: String,
    tx: Sender<Event>,
    needs_settings_refresh: bool,
}

impl<A: ApiTransport> AuthObserver<A> {
    pub fn new(api: A, device_name: String, platform_name: String, tx: Sender<Event>) -> Self {
        Self {
            state: AuthObserverState::default(),
            api,
            device_name,
            platform_name,
            tx,
            needs_settings_refresh: false,
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

        let settings = self
            .api
            .get_device_settings(&device.access_token)
            .or_else(|err| {
                if err.is_unauthorized() {
                    let refreshed = self.api.refresh_device_token(&device.refresh_token)?;
                    device.access_token = refreshed.clone();
                    self.api.get_device_settings(&refreshed)
                } else {
                    Err(err)
                }
            })?;

        self.state.user_access_token = Some(access_token);
        self.state.device_credentials = Some(device.clone());
        self.needs_settings_refresh = false;
        Ok((device, settings))
    }

    fn handle_logout_requested(&mut self) {
        if let Some(token) = self.state.user_access_token.clone() {
            let _ = self.api.logout(&token);
        }

        self.state.user_access_token = None;
        self.state.device_credentials = None;
        self.needs_settings_refresh = false;

        self.tx.send(Event::Logout).ok();
        self.tx
            .send(Event::LogoutResult {
                success: true,
                error: None,
            })
            .ok();
    }

    fn handle_ping(&mut self) {
        if self.state.device_credentials.is_none() || !self.needs_settings_refresh {
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
            .ok_or(crate::error::CoreError::NotAuthenticated)?;

        match self.api.get_device_settings(&credentials.access_token) {
            Err(err) if err.is_unauthorized() => {
                let refreshed = self.api.refresh_device_token(&credentials.refresh_token)?;
                credentials.access_token = refreshed.clone();
                self.state.device_credentials = Some(credentials);
                let settings = self.api.get_device_settings(&refreshed)?;
                Ok(settings)
            }
            Err(err) if err.is_not_found() => {
                log_error("device not found; clearing local auth", Some(&err));
                self.state.user_access_token = None;
                self.state.device_credentials = None;
                self.tx.send(Event::Logout).ok();
                Err(crate::error::CoreError::NotAuthenticated)
            }
            other => other,
        }
    }
}

impl<A: ApiTransport + 'static> Observer for AuthObserver<A> {
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

    fn on_event(&mut self, event: &Event) -> CoreResult<()> {
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
