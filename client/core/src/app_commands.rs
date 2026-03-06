use chrono::Utc;
use serde::Deserialize;

use base64::Engine;

use crate::api_client::ApiClient;
use crate::auth::AuthClient;
use crate::error::{CoreError, CoreResult};
use crate::token_store::TokenStore;

#[derive(Clone, Debug)]
pub struct LoginCommandInput<'a> {
    pub email: &'a str,
    pub password: &'a str,
    pub device_name: &'a str,
    pub platform: &'a str,
}

#[derive(Clone, Debug)]
pub struct LoginCommandResult {
    pub device_id: String,
    pub user_id: String,
}

pub async fn login_and_register_device(
    auth_client: &AuthClient,
    api_client: &ApiClient,
    token_store: &dyn TokenStore,
    input: LoginCommandInput<'_>,
) -> CoreResult<LoginCommandResult> {
    let login = auth_client.login(input.email, input.password).await?;
    let access_token = login.access_token;

    let user_id = parse_jwt_sub(&access_token).ok_or_else(|| {
        CoreError::TokenStore("could not extract user ID from access token".to_string())
    })?;
    auth_client.store_wrapping_key(input.password, &user_id)?;

    let registration = api_client
        .register_device(&access_token, input.device_name, input.platform)
        .await?;
    token_store.set_access_token(&registration.access_token)?;
    token_store.set_refresh_token(&registration.refresh_token)?;
    auth_client
        .fetch_and_decrypt_e2ee_key(&registration.access_token)
        .await?;

    let _ = api_client
        .create_alert_log(
            &registration.access_token,
            &registration.id,
            "login",
            &[
                ("source".to_string(), "app_command".to_string()),
                ("platform".to_string(), input.platform.to_string()),
            ],
            Utc::now(),
        )
        .await;

    Ok(LoginCommandResult {
        device_id: registration.id,
        user_id,
    })
}

pub async fn logout_and_clear_tokens(
    auth_client: &AuthClient,
    token_store: &dyn TokenStore,
) -> CoreResult<()> {
    logout_and_clear_tokens_with_alert(auth_client, None, token_store, None, &[]).await
}

pub async fn logout_and_clear_tokens_with_alert(
    auth_client: &AuthClient,
    api_client: Option<&ApiClient>,
    token_store: &dyn TokenStore,
    device_id: Option<&str>,
    metadata: &[(String, String)],
) -> CoreResult<()> {
    if let (Some(api_client), Some(device_id), Some(access_token)) =
        (api_client, device_id, token_store.get_access_token()?)
    {
        let _ = api_client
            .create_alert_log(&access_token, device_id, "logout", metadata, Utc::now())
            .await;
    }

    auth_client.logout().await?;
    clear_local_tokens(token_store)
}

pub fn clear_local_tokens(token_store: &dyn TokenStore) -> CoreResult<()> {
    token_store.clear_access_token()?;
    token_store.clear_refresh_token()?;
    token_store.clear_e2ee_key()?;
    token_store.clear_wrapping_key()?;
    Ok(())
}

#[derive(Deserialize)]
struct JwtClaims {
    sub: Option<String>,
}

/// Extract the `sub` claim (user ID) from a JWT without verifying signature.
pub fn parse_jwt_sub(token: &str) -> Option<String> {
    let payload_segment = token.split('.').nth(1)?;
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload_segment)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(payload_segment))
        .ok()?;
    let claims: JwtClaims = serde_json::from_slice(&payload).ok()?;
    claims.sub.filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{clear_local_tokens, parse_jwt_sub};
    use crate::token_store::{MemoryTokenStore, TokenStore};

    #[test]
    fn parse_jwt_sub_extracts_value() {
        let token = "aaa.eyJzdWIiOiJ1c2VyLTEyMyJ9.bbb";
        assert_eq!(parse_jwt_sub(token), Some("user-123".to_string()));
    }

    #[test]
    fn clear_local_tokens_removes_everything() {
        let store = MemoryTokenStore::new();
        store.set_access_token("access").expect("set access");
        store.set_refresh_token("refresh").expect("set refresh");
        store.set_e2ee_key(&[1u8; 32]).expect("set e2ee");
        store.set_wrapping_key(&[2u8; 32]).expect("set wrapping");

        clear_local_tokens(&store).expect("clear");

        assert!(store.get_access_token().expect("access").is_none());
        assert!(store.get_refresh_token().expect("refresh").is_none());
        assert!(store.get_e2ee_key().expect("e2ee").is_none());
        assert!(store.get_wrapping_key().expect("wrap").is_none());
    }
}
