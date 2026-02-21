use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;

use crate::DEFAULT_BASE_API_URL;
use crate::error::{CoreError, CoreResult};
use crate::models::{LoginRequest, TokenResponse};
use crate::token_store::TokenStore;

#[derive(Clone, Debug)]
pub struct AuthClientConfig {
    pub base_url: String,
    pub timeout: Duration,
}

impl Default for AuthClientConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_API_URL.to_string(),
            timeout: Duration::from_secs(15),
        }
    }
}

#[derive(Clone)]
pub struct AuthClient {
    client: reqwest::Client,
    config: AuthClientConfig,
    token_store: Arc<dyn TokenStore>,
}

impl AuthClient {
    pub fn new(token_store: Arc<dyn TokenStore>) -> CoreResult<Self> {
        Self::with_config(token_store, AuthClientConfig::default())
    }

    pub fn with_config(
        token_store: Arc<dyn TokenStore>,
        config: AuthClientConfig,
    ) -> CoreResult<Self> {
        let client = reqwest::Client::builder()
            .cookie_store(true)
            .timeout(config.timeout)
            .build()?;

        Ok(Self {
            client,
            config,
            token_store,
        })
    }

    pub fn get_access_token(&self) -> CoreResult<Option<String>> {
        self.token_store.get_access_token()
    }

    pub async fn login(&self, email: &str, password: &str) -> CoreResult<TokenResponse> {
        let payload = LoginRequest { email, password };
        let url = format!("{}/login", self.config.base_url);
        let response = self.client.post(url).json(&payload).send().await?;

        let parsed: TokenResponse = decode_response(response).await?;
        self.token_store.set_access_token(&parsed.access_token)?;
        Ok(parsed)
    }

    pub async fn refresh_access_token(&self) -> CoreResult<TokenResponse> {
        let url = format!("{}/token", self.config.base_url);
        let response = self.client.post(url).send().await?;

        let parsed: TokenResponse = decode_response(response).await?;
        self.token_store.set_access_token(&parsed.access_token)?;
        Ok(parsed)
    }

    pub async fn logout(&self) -> CoreResult<()> {
        let url = format!("{}/logout", self.config.base_url);
        let response = self.client.post(url).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(CoreError::UnexpectedResponse { status, body });
        }

        self.token_store.clear_access_token()?;
        Ok(())
    }
}

async fn decode_response<T: DeserializeOwned>(response: reqwest::Response) -> CoreResult<T> {
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(CoreError::UnexpectedResponse { status, body });
    }

    Ok(response.json::<T>().await?)
}
