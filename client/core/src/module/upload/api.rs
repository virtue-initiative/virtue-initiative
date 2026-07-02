use std::time::{Duration, Instant};

use crate::api::ApiTransport;
use crate::error::{CoreError, CoreResult};
use crate::model::{BatchUpload, DeviceCredentials, LogEntry};

const HASH_TOKEN_MAX_AGE: Duration = Duration::from_secs(55 * 60);

pub struct UploadApi<A: ApiTransport + Clone> {
    pub(super) api: A,
    credentials: Option<DeviceCredentials>,
    hash_token_cache: Option<(String, Instant)>,
}

impl<A: ApiTransport + Clone> UploadApi<A> {
    pub fn new(api: A) -> Self {
        Self {
            api,
            credentials: None,
            hash_token_cache: None,
        }
    }

    pub fn set_credentials(&mut self, creds: Option<DeviceCredentials>) {
        self.credentials = creds;
        self.hash_token_cache = None;
    }

    pub fn credentials(&self) -> Option<&DeviceCredentials> {
        self.credentials.as_ref()
    }

    pub fn is_authenticated(&self) -> bool {
        self.credentials.is_some()
    }

    pub fn upload_batch(&self, batch: &BatchUpload) -> CoreResult<()> {
        let creds = self
            .credentials
            .as_ref()
            .ok_or(CoreError::NotAuthenticated)?;
        self.api
            .upload_batch(&creds.refresh_token, batch)
            .map(|_| ())
    }

    pub fn upload_hash(
        &mut self,
        hash_base_url: Option<&str>,
        content_hash: &[u8; 32],
    ) -> CoreResult<()> {
        let hash_jwt = self.ensure_hash_token()?;
        self.api.upload_hash(hash_base_url, &hash_jwt, content_hash)
    }

    pub fn upload_log(&self, entry: &LogEntry) -> CoreResult<()> {
        let creds = self
            .credentials
            .as_ref()
            .ok_or(CoreError::NotAuthenticated)?;
        self.api.upload_log(&creds.refresh_token, entry).map(|_| ())
    }

    fn ensure_hash_token(&mut self) -> CoreResult<String> {
        let creds = self
            .credentials
            .as_ref()
            .ok_or(CoreError::NotAuthenticated)?;
        let refresh_token = creds.refresh_token.clone();

        let needs_refresh = match &self.hash_token_cache {
            None => true,
            Some((_, fetched_at)) => fetched_at.elapsed() >= HASH_TOKEN_MAX_AGE,
        };

        if needs_refresh {
            let token = self.api.get_hash_token(&refresh_token)?;
            self.hash_token_cache = Some((token.clone(), Instant::now()));
            Ok(token)
        } else {
            Ok(self.hash_token_cache.as_ref().unwrap().0.clone())
        }
    }
}

#[cfg(feature = "testing")]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::UploadKind;
    use crate::error::CoreError;
    use crate::model::{DeviceCredentials, LogEntry};
    use crate::testing::api::MockApiClient;

    fn test_entry() -> LogEntry {
        LogEntry {
            ts: 1000,
            risk: None,
            event: UploadKind::Alert {
                message: "test-alert".to_string(),
            },
        }
    }

    fn test_creds() -> DeviceCredentials {
        DeviceCredentials {
            device_id: "test-device".into(),
            refresh_token: "test-refresh".into(),
        }
    }

    #[test]
    fn upload_without_credentials_returns_not_authenticated() {
        let mock = MockApiClient::new();
        let mut api = UploadApi::new(mock);
        let result = api.upload_log(&test_entry());
        assert!(matches!(result, Err(CoreError::NotAuthenticated)));
    }

    #[test]
    fn upload_succeeds_on_first_attempt() {
        let mock = MockApiClient::new();
        let inspector = mock.clone();
        let mut api = UploadApi::new(mock);
        api.set_credentials(Some(test_creds()));
        api.upload_log(&test_entry()).expect("upload must succeed");
        let state = inspector.state();
        assert_eq!(state.log_uploads.len(), 1);
        assert_eq!(state.log_uploads[0].device_refresh_token, "test-refresh");
    }

    #[test]
    fn upload_does_not_retry_on_error() {
        let mock = MockApiClient::new();
        let inspector = mock.clone();
        mock.program_log(Err(CoreError::HttpStatus {
            status: 500,
            message: "server error".into(),
        }));
        let mut api = UploadApi::new(mock);
        api.set_credentials(Some(test_creds()));
        let result = api.upload_log(&test_entry());
        assert!(result.is_err());
        let state = inspector.state();
        assert_eq!(state.log_uploads.len(), 1);
    }

    #[test]
    fn hash_token_is_fetched_and_cached() {
        let mock = MockApiClient::new();
        let inspector = mock.clone();
        let mut api = UploadApi::new(mock);
        api.set_credentials(Some(test_creds()));
        api.upload_hash(None, &[0u8; 32])
            .expect("hash upload must succeed");
        api.upload_hash(None, &[0u8; 32])
            .expect("second hash upload must succeed");
        let state = inspector.state();
        assert_eq!(
            state.get_hash_token_calls.len(),
            1,
            "token should be fetched once and cached"
        );
        assert_eq!(state.hash_uploads.len(), 2);
    }
}
