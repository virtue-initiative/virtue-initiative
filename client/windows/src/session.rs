use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::runtime::Runtime;

use crate::config::{ClientPaths, load_state, save_state};
use virtue_core::{
    ApiClient, AuthClient, FileTokenStore, LoginCommandInput, TokenStore,
    login_and_register_device, logout_and_clear_tokens_with_alert,
};

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
    pub email: Option<String>,
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
            email: state.email,
        })
    }

    pub fn login_blocking(
        &self,
        runtime: &Runtime,
        email: &str,
        password: &str,
        device_name: &str,
    ) -> Result<String> {
        runtime.block_on(async {
            let result = login_and_register_device(
                &self.auth_client,
                &self.api_client,
                self.token_store.as_ref(),
                LoginCommandInput {
                    email,
                    password,
                    device_name,
                    platform: "windows",
                },
            )
            .await
            .context("login failed")?;

            let mut state = load_state(&self.paths.state_file)?;
            state.device_id = Some(result.device_id.clone());
            state.monitoring_enabled = true;
            state.email = Some(email.to_string());
            save_state(&self.paths.state_file, &state)?;

            Ok::<String, anyhow::Error>(result.device_id)
        })
    }

    pub fn logout_blocking(&self, runtime: &Runtime) -> Result<()> {
        runtime.block_on(async {
            let mut state = load_state(&self.paths.state_file)?;
            let metadata = vec![("source".to_string(), "windows_ui".to_string())];
            let _ = logout_and_clear_tokens_with_alert(
                &self.auth_client,
                Some(&self.api_client),
                self.token_store.as_ref(),
                state.device_id.as_deref(),
                &metadata,
            )
            .await;

            state.monitoring_enabled = false;
            state.device_id = None;
            state.email = None;
            save_state(&self.paths.state_file, &state)?;

            Ok::<(), anyhow::Error>(())
        })
    }
}
