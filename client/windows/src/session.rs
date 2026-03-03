use std::sync::Arc;

use anyhow::{Context, Result};
use base64::Engine;
use serde::Deserialize;
use tokio::runtime::Runtime;

use virtue_client_core::{AuthClient, FileTokenStore, TokenStore, derive_key};

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
    ) -> Result<String> {
        runtime.block_on(async {
            self.auth_client
                .login(email, password)
                .await
                .context("login failed")?;

            let access_token = self
                .token_store
                .get_access_token()?
                .context("missing access token after login")?;

            let user_id = parse_jwt_sub(&access_token)
                .context("could not extract user ID from access token")?;
            let e2ee_key = derive_key(password, &user_id);
            self.token_store.set_e2ee_key(&e2ee_key)?;

            let registration = self
                .api_client
                .register_device(&access_token, device_name)
                .await
                .context("device registration failed")?;

            let mut state = load_state(&self.paths.state_file)?;
            state.device_id = Some(registration.id.clone());
            state.monitoring_enabled = true;
            save_state(&self.paths.state_file, &state)?;

            Ok::<String, anyhow::Error>(registration.id)
        })
    }

    pub fn logout_blocking(&self, runtime: &Runtime) -> Result<()> {
        runtime.block_on(async {
            let mut state = load_state(&self.paths.state_file)?;
            let _ = self.auth_client.logout().await;
            self.token_store.clear_access_token()?;
            self.token_store.clear_refresh_token()?;
            self.token_store.clear_e2ee_key()?;

            state.monitoring_enabled = false;
            state.device_id = None;
            save_state(&self.paths.state_file, &state)?;

            Ok::<(), anyhow::Error>(())
        })
    }
}

#[derive(Deserialize)]
struct JwtClaims {
    sub: Option<String>,
}

fn parse_jwt_sub(token: &str) -> Option<String> {
    let payload_segment = token.split('.').nth(1)?;
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload_segment)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(payload_segment))
        .ok()?;
    let claims: JwtClaims = serde_json::from_slice(&payload).ok()?;
    claims.sub.filter(|s| !s.is_empty())
}
