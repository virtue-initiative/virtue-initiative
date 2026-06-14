use crate::api::ApiTransport;
use crate::error::{CoreError, CoreResult};
use crate::model::{BatchUpload, DeviceCredentials, LogEntry};

pub struct UploadApi<A: ApiTransport + Clone> {
    pub(super) api: A,
    credentials: Option<DeviceCredentials>,
}

impl<A: ApiTransport + Clone> UploadApi<A> {
    pub fn new(api: A) -> Self {
        Self {
            api,
            credentials: None,
        }
    }

    pub fn set_credentials(&mut self, creds: Option<DeviceCredentials>) {
        self.credentials = creds;
    }

    pub fn credentials(&self) -> Option<&DeviceCredentials> {
        self.credentials.as_ref()
    }

    pub fn is_authenticated(&self) -> bool {
        self.credentials.is_some()
    }

    pub fn upload_batch(&mut self, batch: &BatchUpload) -> CoreResult<()> {
        self.with_token_retry(|api, token| api.upload_batch(token, batch).map(|_| ()))
    }

    pub fn upload_hash(
        &mut self,
        hash_base_url: Option<&str>,
        content_hash: &[u8; 32],
    ) -> CoreResult<()> {
        self.with_token_retry(|api, token| api.upload_hash(hash_base_url, token, content_hash))
    }

    pub fn upload_log(&mut self, entry: &LogEntry) -> CoreResult<()> {
        self.with_token_retry(|api, token| api.upload_log(token, entry).map(|_| ()))
    }

    fn with_token_retry<T, F>(&mut self, mut operation: F) -> CoreResult<T>
    where
        F: FnMut(&A, &str) -> CoreResult<T>,
    {
        let creds = self
            .credentials
            .as_ref()
            .ok_or(CoreError::NotAuthenticated)?;
        let access_token = creds.access_token.clone();
        let refresh_token = creds.refresh_token.clone();

        match operation(&self.api, &access_token) {
            Ok(value) => Ok(value),
            Err(err) if err.is_unauthorized() => {
                let refreshed = self.api.refresh_device_token(&refresh_token)?;
                if let Some(creds) = self.credentials.as_mut() {
                    creds.access_token = refreshed.clone();
                }
                operation(&self.api, &refreshed)
            }
            Err(err) => Err(err),
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
            access_token: "test-access".into(),
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
        assert_eq!(state.refresh_device_token_calls.len(), 0);
    }

    #[test]
    fn upload_retries_after_401_and_succeeds() {
        let mock = MockApiClient::new();
        let inspector = mock.clone();
        mock.program_log(Err(CoreError::HttpStatus {
            status: 401,
            message: "unauthorized".into(),
        }));
        mock.program_refresh_device_token(Ok("new-token".into()));
        let mut api = UploadApi::new(mock);
        api.set_credentials(Some(test_creds()));
        api.upload_log(&test_entry())
            .expect("upload must succeed after retry");
        let state = inspector.state();
        assert_eq!(state.refresh_device_token_calls.len(), 1);
        assert_eq!(
            state.log_uploads.len(),
            2,
            "failed attempt + successful retry"
        );
    }

    #[test]
    fn upload_does_not_retry_non_401_error() {
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
        assert_eq!(state.refresh_device_token_calls.len(), 0);
        assert_eq!(state.log_uploads.len(), 1);
    }
}
