use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use serde_json::json;
use tokio::runtime::Runtime;

use bepure_client_core::{
    AuthClient, FileTokenStore, TokenStore, resolve_capture_interval_seconds,
};

use crate::api::ApiClient;
use crate::config::{ClientPaths, load_state, save_state};

#[derive(Clone)]
pub struct SessionManager {
    pub paths: ClientPaths,
    pub token_store: Arc<dyn TokenStore>,
    pub auth_client: AuthClient,
    pub api_client: ApiClient,
}

#[derive(Clone, Debug)]
pub struct SessionStatus {
    pub logged_in: bool,
    pub device_id: Option<String>,
}

impl SessionManager {
    pub fn new() -> Result<Self> {
        let paths = ClientPaths::discover()?;
        paths.ensure_dirs()?;

        let token_store: Arc<dyn TokenStore> = Arc::new(FileTokenStore::new(&paths.token_file));
        let auth_client = AuthClient::new(token_store.clone())?;
        let api_client = ApiClient::new()?;

        Ok(Self {
            paths,
            token_store,
            auth_client,
            api_client,
        })
    }

    pub fn status(&self) -> Result<SessionStatus> {
        let state = load_state(&self.paths.state_file)?;
        let logged_in = self.token_store.get_access_token()?.is_some() && state.device_id.is_some();

        Ok(SessionStatus {
            logged_in,
            device_id: state.device_id,
        })
    }

    pub fn login_blocking(
        &self,
        runtime: &Runtime,
        email: &str,
        password: &str,
        device_name: &str,
        interval_seconds: u64,
    ) -> Result<String> {
        runtime.block_on(async {
            let effective_interval_seconds = resolve_capture_interval_seconds(interval_seconds);

            self.auth_client
                .login(email, password)
                .await
                .context("login failed")?;

            let access_token = self
                .token_store
                .get_access_token()?
                .context("missing access token after login")?;

            let registration = self
                .api_client
                .register_device(
                    &access_token,
                    device_name,
                    "windows",
                    effective_interval_seconds,
                )
                .await
                .context("device registration failed")?;

            let mut state = load_state(&self.paths.state_file)?;
            state.device_id = Some(registration.id.clone());
            state.monitoring_enabled = true;
            state.capture_interval_seconds = effective_interval_seconds;
            save_state(&self.paths.state_file, &state)?;

            Ok::<String, anyhow::Error>(registration.id)
        })
    }

    pub fn logout_blocking(&self, runtime: &Runtime) -> Result<()> {
        runtime.block_on(async {
            let mut state = load_state(&self.paths.state_file)?;
            let access_token = self.token_store.get_access_token()?;

            if let (Some(token), Some(device_id)) =
                (access_token.as_deref(), state.device_id.as_deref())
            {
                let mut metadata = BTreeMap::new();
                metadata.insert("reason".to_string(), json!("user_logout"));
                let _ = self
                    .api_client
                    .send_log(token, "manual_override", device_id, None, metadata)
                    .await;
            }

            let _ = self.auth_client.logout().await;
            self.token_store.clear_access_token()?;
            self.token_store.clear_refresh_token()?;

            state.monitoring_enabled = false;
            state.device_id = None;
            save_state(&self.paths.state_file, &state)?;

            Ok::<(), anyhow::Error>(())
        })
    }
}
