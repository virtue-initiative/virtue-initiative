use std::any::Any;
use std::sync::mpsc::Sender;

use crate::api::ApiTransport;
use crate::auth::Auth;
use crate::error::CoreResult;
use crate::events::{Event, Observer, StateType, log_error};
use crate::storage::FileStateStore;

pub struct AuthObserver<A: ApiTransport> {
    pub auth: Auth,
    pub(crate) api: A,
    device_name: String,
    platform_name: String,
    storage: FileStateStore,
    tx: Sender<Event>,
    needs_settings_refresh: bool,
}

impl<A: ApiTransport> AuthObserver<A> {
    pub fn new(
        auth: Auth,
        api: A,
        device_name: String,
        platform_name: String,
        storage: FileStateStore,
        tx: Sender<Event>,
    ) -> Self {
        let needs_settings_refresh = auth.is_authenticated();
        Self {
            auth,
            api,
            device_name,
            platform_name,
            storage,
            tx,
            needs_settings_refresh,
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
        self.storage.clear_stop_intent()?;

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

        self.auth.set_login(access_token, device.clone());
        self.auth.persist(&self.storage)?;
        self.needs_settings_refresh = false;
        Ok((device, settings))
    }

    fn handle_logout_requested(&mut self) {
        if let Some(token) = self.auth.user_access_token.clone() {
            let _ = self.api.logout(&token);
        }
        self.auth.clear();
        let result = self.auth.persist(&self.storage);
        self.storage.clear_stop_intent().ok();
        self.needs_settings_refresh = false;
        if result.is_ok() {
            self.tx.send(Event::Logout).ok();
        }
        self.tx
            .send(Event::LogoutResult {
                success: result.is_ok(),
                error: result.err().map(|e| e.to_string()),
            })
            .ok();
    }

    fn handle_ping(&mut self) {
        if !self.auth.is_authenticated() || !self.needs_settings_refresh {
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
            .auth
            .device_credentials
            .clone()
            .ok_or(crate::error::CoreError::NotAuthenticated)?;

        match self.api.get_device_settings(&credentials.access_token) {
            Err(err) if err.is_unauthorized() => {
                let refreshed = self.api.refresh_device_token(&credentials.refresh_token)?;
                credentials.access_token = refreshed.clone();
                self.auth.set_credentials(credentials);
                self.auth.persist(&self.storage)?;
                let settings = self.api.get_device_settings(&refreshed)?;
                Ok(settings)
            }
            Err(err) if err.is_not_found() => {
                log_error("device not found; clearing local auth", Some(&err));
                self.auth.clear();
                self.auth.persist(&self.storage)?;
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
        Ok(serde_json::Value::Null)
    }

    fn load_state(&mut self, _state: StateType) -> CoreResult<()> {
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
            _ => {}
        }
        Ok(())
    }
}
