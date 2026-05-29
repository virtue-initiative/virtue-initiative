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
