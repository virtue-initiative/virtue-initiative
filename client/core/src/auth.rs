use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use reqwest::header::{COOKIE, HeaderValue, SET_COOKIE};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::crypto::{derive_key, decrypt};
use crate::error::{CoreError, CoreResult};
use crate::models::{E2EEKeyResponse, LoginRequest, TokenResponse};
use crate::resolve_base_api_url;
use crate::token_store::TokenStore;

#[derive(Clone, Debug)]
pub struct AuthClientConfig {
    pub base_url: String,
    pub timeout: Duration,
}

impl Default for AuthClientConfig {
    fn default() -> Self {
        Self {
            base_url: resolve_base_api_url(),
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
        let set_cookie_headers = collect_set_cookie_headers(&response);

        let parsed: TokenResponse = decode_response(response).await?;
        self.token_store.set_access_token(&parsed.access_token)?;
        if let Some(refresh_token) = extract_refresh_token(&set_cookie_headers) {
            self.token_store.set_refresh_token(&refresh_token)?;
        }
        Ok(parsed)
    }

    pub async fn refresh_access_token(&self) -> CoreResult<TokenResponse> {
        let url = format!("{}/token", self.config.base_url);
        let mut request = self.client.post(url);
        if let Some(refresh_token) = self.token_store.get_refresh_token()? {
            request = request.header(COOKIE, format!("refresh_token={refresh_token}"));
        }
        let response = request.send().await?;
        let set_cookie_headers = collect_set_cookie_headers(&response);

        let parsed: TokenResponse = decode_response(response).await?;
        self.token_store.set_access_token(&parsed.access_token)?;
        if let Some(refresh_token) = extract_refresh_token(&set_cookie_headers) {
            self.token_store.set_refresh_token(&refresh_token)?;
        }
        Ok(parsed)
    }

    pub async fn refresh_access_token_if_needed(
        &self,
        min_valid_for: Duration,
    ) -> CoreResult<Option<TokenResponse>> {
        let Some(access_token) = self.token_store.get_access_token()? else {
            return Ok(None);
        };

        if !token_expires_within(&access_token, min_valid_for) {
            return Ok(None);
        }

        if self.token_store.get_refresh_token()?.is_none() {
            return Err(CoreError::TokenStore(
                "missing refresh token; please sign in again".to_string(),
            ));
        }

        let refreshed = self.refresh_access_token().await?;
        Ok(Some(refreshed))
    }

    pub async fn logout(&self) -> CoreResult<()> {
        let url = format!("{}/logout", self.config.base_url);
        let mut request = self.client.post(url);
        if let Some(refresh_token) = self.token_store.get_refresh_token()? {
            request = request.header(COOKIE, format!("refresh_token={refresh_token}"));
        }
        let response = request.send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(CoreError::UnexpectedResponse { status, body });
        }

        self.token_store.clear_access_token()?;
        self.token_store.clear_refresh_token()?;
        Ok(())
    }

    /// Derive a wrapping key from the login password and user ID, and persist it in the token store.
    /// Call this once at login; the wrapping key is used by `fetch_and_decrypt_e2ee_key` on restart.
    pub fn store_wrapping_key(&self, password: &str, user_id: &str) -> CoreResult<()> {
        let wrapping_key = derive_key(password, user_id);
        self.token_store.set_wrapping_key(&wrapping_key)
    }

    /// Fetch the encrypted E2EE key from `GET /e2ee`, decrypt it with the stored wrapping key,
    /// and persist the result. Call at login and on each daemon restart.
    pub async fn fetch_and_decrypt_e2ee_key(&self, access_token: &str) -> CoreResult<[u8; 32]> {
        let wrapping_key = self.token_store.get_wrapping_key()?.ok_or_else(|| {
            CoreError::TokenStore(
                "wrapping key not found; please sign in again".to_string(),
            )
        })?;

        let url = format!("{}/e2ee", self.config.base_url);
        let response = self
            .client
            .get(url)
            .bearer_auth(access_token)
            .send()
            .await?;

        let e2ee_resp: E2EEKeyResponse = decode_response(response).await?;

        let encrypted_b64 = e2ee_resp.encrypted_e2ee_key.ok_or_else(|| {
            CoreError::TokenStore("no E2EE key stored on server; please sign in via the web app first".to_string())
        })?;

        let encrypted = base64::engine::general_purpose::STANDARD
            .decode(&encrypted_b64)
            .map_err(|e| CoreError::Crypto(format!("base64 decode failed: {e}")))?;

        let raw = decrypt(&wrapping_key, &encrypted)?;

        if raw.len() != 32 {
            return Err(CoreError::Crypto(format!(
                "expected 32-byte E2EE key, got {}",
                raw.len()
            )));
        }

        let mut key = [0u8; 32];
        key.copy_from_slice(&raw);
        self.token_store.set_e2ee_key(&key)?;
        Ok(key)
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

#[derive(Debug, Deserialize)]
struct JwtClaims {
    exp: Option<u64>,
}

fn token_expires_within(token: &str, threshold: Duration) -> bool {
    let Some(expiry) = parse_jwt_expiry(token) else {
        return false;
    };

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    expiry <= now.saturating_add(threshold.as_secs())
}

fn parse_jwt_expiry(token: &str) -> Option<u64> {
    let payload_segment = token.split('.').nth(1)?;
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload_segment)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(payload_segment))
        .ok()?;

    let claims = serde_json::from_slice::<JwtClaims>(&payload).ok()?;
    claims.exp
}

fn collect_set_cookie_headers(response: &reqwest::Response) -> Vec<HeaderValue> {
    response
        .headers()
        .get_all(SET_COOKIE)
        .iter()
        .cloned()
        .collect()
}

fn extract_refresh_token(set_cookie_headers: &[HeaderValue]) -> Option<String> {
    set_cookie_headers
        .iter()
        .filter_map(|header| header.to_str().ok())
        .find_map(parse_refresh_token_cookie)
}

fn parse_refresh_token_cookie(set_cookie: &str) -> Option<String> {
    let prefix = "refresh_token=";
    if !set_cookie.starts_with(prefix) {
        return None;
    }

    let value = set_cookie[prefix.len()..]
        .split(';')
        .next()
        .map(str::trim)
        .unwrap_or_default();
    if value.is_empty() {
        return None;
    }

    Some(value.to_string())
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use base64::Engine;
    use reqwest::header::HeaderValue;

    use super::{extract_refresh_token, token_expires_within};

    #[test]
    fn extracts_refresh_token_from_set_cookie_header() {
        let headers = vec![
            HeaderValue::from_static("session=ignore; Path=/"),
            HeaderValue::from_static("refresh_token=test-refresh-token; Path=/; HttpOnly"),
        ];

        assert_eq!(
            extract_refresh_token(&headers),
            Some("test-refresh-token".to_string())
        );
    }

    #[test]
    fn detects_expiring_tokens() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let payload = format!(r#"{{"exp":{}}}"#, now + 30);
        let payload_segment =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload.as_bytes());
        let token = format!("header.{payload_segment}.signature");

        assert!(token_expires_within(&token, Duration::from_secs(60)));
        assert!(!token_expires_within(&token, Duration::from_secs(5)));
    }
}
