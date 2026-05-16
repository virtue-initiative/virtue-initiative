use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::api::{ApiTransport, UploadedBatchResponse, UploadedLogResponse};
use crate::config::Config;
use crate::error::CoreResult;
use crate::model::{BatchUpload, DeviceCredentials, DeviceSettings, LogEntry};

/// Mock `ApiTransport` impl that records every call and serves either a
/// programmed canned response or a sensible default success.
///
/// Cheap to clone — the underlying state is `Arc<Mutex<_>>`, so handing a
/// clone to the test and moving another into the service is the intended
/// usage:
///
/// ```ignore
/// let mock = MockApiClient::new();
/// let inspector = mock.clone();
/// let svc = MonitorService::setup_with_api(cfg, platform, mock)?;
/// // ... drive the service ...
/// assert_eq!(inspector.state().batch_uploads.len(), 2);
/// ```
#[derive(Clone)]
pub struct MockApiClient {
    state: Arc<Mutex<MockApiState>>,
}

pub struct MockApiState {
    // --- recordings ---
    pub login_calls: Vec<(String, String)>,
    pub logout_calls: Vec<String>,
    pub register_device_calls: Vec<RegisterDeviceCall>,
    pub get_device_settings_calls: Vec<String>,
    pub refresh_device_token_calls: Vec<String>,
    pub batch_uploads: Vec<BatchCall>,
    pub log_uploads: Vec<LogCall>,
    pub hash_uploads: Vec<HashCall>,
    pub reconfigure_calls: Vec<String>,

    // --- canned responses (FIFO per method) ---
    pub login_responses: VecDeque<CoreResult<String>>,
    pub logout_responses: VecDeque<CoreResult<()>>,
    pub register_device_responses: VecDeque<CoreResult<DeviceCredentials>>,
    pub get_device_settings_responses: VecDeque<CoreResult<DeviceSettings>>,
    pub refresh_device_token_responses: VecDeque<CoreResult<String>>,
    pub batch_responses: VecDeque<CoreResult<UploadedBatchResponse>>,
    pub log_responses: VecDeque<CoreResult<UploadedLogResponse>>,
    pub hash_responses: VecDeque<CoreResult<()>>,

    // --- default values used when the canned queue is empty ---
    pub default_device_id: String,
    pub default_access_token: String,
    pub default_refresh_token: String,
    pub default_device_settings: DeviceSettings,
    batch_id_counter: u64,
    log_id_counter: u64,
}

#[derive(Debug, Clone)]
pub struct RegisterDeviceCall {
    pub access_token: String,
    pub name: String,
    pub platform: String,
}

#[derive(Debug, Clone)]
pub struct BatchCall {
    pub device_access_token: String,
    pub batch: BatchUpload,
}

#[derive(Debug, Clone)]
pub struct LogCall {
    pub device_access_token: String,
    pub log: LogEntry,
}

#[derive(Debug, Clone)]
pub struct HashCall {
    pub hash_base_url: Option<String>,
    pub device_access_token: String,
    pub content_hash: [u8; 32],
}

impl Default for MockApiState {
    fn default() -> Self {
        Self {
            login_calls: Vec::new(),
            logout_calls: Vec::new(),
            register_device_calls: Vec::new(),
            get_device_settings_calls: Vec::new(),
            refresh_device_token_calls: Vec::new(),
            batch_uploads: Vec::new(),
            log_uploads: Vec::new(),
            hash_uploads: Vec::new(),
            reconfigure_calls: Vec::new(),

            login_responses: VecDeque::new(),
            logout_responses: VecDeque::new(),
            register_device_responses: VecDeque::new(),
            get_device_settings_responses: VecDeque::new(),
            refresh_device_token_responses: VecDeque::new(),
            batch_responses: VecDeque::new(),
            log_responses: VecDeque::new(),
            hash_responses: VecDeque::new(),

            default_device_id: "mock-device".to_string(),
            default_access_token: "mock-access-token".to_string(),
            default_refresh_token: "mock-refresh-token".to_string(),
            default_device_settings: DeviceSettings {
                device_id: "mock-device".to_string(),
                name: "mock device".to_string(),
                platform: "mock".to_string(),
                owner: None,
                partners: Vec::new(),
                hash_base_url: None,
            },
            batch_id_counter: 0,
            log_id_counter: 0,
        }
    }
}

impl MockApiClient {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(MockApiState::default())),
        }
    }

    /// Lock the state and return a guard. Use this to inspect recordings or
    /// program canned responses.
    pub fn state(&self) -> std::sync::MutexGuard<'_, MockApiState> {
        self.state.lock().expect("MockApiClient state poisoned")
    }

    // --- convenience programming helpers (each pushes one canned response) ---

    pub fn program_login(&self, response: CoreResult<String>) {
        self.state().login_responses.push_back(response);
    }

    pub fn program_register_device(&self, response: CoreResult<DeviceCredentials>) {
        self.state().register_device_responses.push_back(response);
    }

    pub fn program_get_device_settings(&self, response: CoreResult<DeviceSettings>) {
        self.state()
            .get_device_settings_responses
            .push_back(response);
    }

    pub fn program_refresh_device_token(&self, response: CoreResult<String>) {
        self.state()
            .refresh_device_token_responses
            .push_back(response);
    }

    pub fn program_batch(&self, response: CoreResult<UploadedBatchResponse>) {
        self.state().batch_responses.push_back(response);
    }

    pub fn program_log(&self, response: CoreResult<UploadedLogResponse>) {
        self.state().log_responses.push_back(response);
    }

    pub fn program_hash(&self, response: CoreResult<()>) {
        self.state().hash_responses.push_back(response);
    }
}

impl Default for MockApiClient {
    fn default() -> Self {
        Self::new()
    }
}

impl ApiTransport for MockApiClient {
    fn reconfigure(&mut self, config: &Config) -> CoreResult<()> {
        self.state()
            .reconfigure_calls
            .push(config.api_base_url.clone());
        Ok(())
    }

    fn login(&self, username: &str, password: &str) -> CoreResult<String> {
        let mut state = self.state();
        state
            .login_calls
            .push((username.to_string(), password.to_string()));
        if let Some(canned) = state.login_responses.pop_front() {
            canned
        } else {
            Ok(state.default_access_token.clone())
        }
    }

    fn logout(&self, access_token: &str) -> CoreResult<()> {
        let mut state = self.state();
        state.logout_calls.push(access_token.to_string());
        if let Some(canned) = state.logout_responses.pop_front() {
            canned
        } else {
            Ok(())
        }
    }

    fn register_device(
        &self,
        access_token: &str,
        name: &str,
        platform: &str,
    ) -> CoreResult<DeviceCredentials> {
        let mut state = self.state();
        state.register_device_calls.push(RegisterDeviceCall {
            access_token: access_token.to_string(),
            name: name.to_string(),
            platform: platform.to_string(),
        });
        if let Some(canned) = state.register_device_responses.pop_front() {
            canned
        } else {
            Ok(DeviceCredentials {
                device_id: state.default_device_id.clone(),
                access_token: state.default_access_token.clone(),
                refresh_token: state.default_refresh_token.clone(),
            })
        }
    }

    fn get_device_settings(&self, device_access_token: &str) -> CoreResult<DeviceSettings> {
        let mut state = self.state();
        state
            .get_device_settings_calls
            .push(device_access_token.to_string());
        if let Some(canned) = state.get_device_settings_responses.pop_front() {
            canned
        } else {
            Ok(state.default_device_settings.clone())
        }
    }

    fn refresh_device_token(&self, refresh_token: &str) -> CoreResult<String> {
        let mut state = self.state();
        state
            .refresh_device_token_calls
            .push(refresh_token.to_string());
        if let Some(canned) = state.refresh_device_token_responses.pop_front() {
            canned
        } else {
            Ok(state.default_access_token.clone())
        }
    }

    fn upload_batch(
        &self,
        device_access_token: &str,
        batch: &BatchUpload,
    ) -> CoreResult<UploadedBatchResponse> {
        let mut state = self.state();
        state.batch_uploads.push(BatchCall {
            device_access_token: device_access_token.to_string(),
            batch: batch.clone(),
        });
        if let Some(canned) = state.batch_responses.pop_front() {
            canned
        } else {
            state.batch_id_counter += 1;
            Ok(UploadedBatchResponse {
                id: format!("mock-batch-{}", state.batch_id_counter),
            })
        }
    }

    fn upload_log(
        &self,
        device_access_token: &str,
        log: &LogEntry,
    ) -> CoreResult<UploadedLogResponse> {
        let mut state = self.state();
        state.log_uploads.push(LogCall {
            device_access_token: device_access_token.to_string(),
            log: log.clone(),
        });
        if let Some(canned) = state.log_responses.pop_front() {
            canned
        } else {
            state.log_id_counter += 1;
            Ok(UploadedLogResponse {
                id: format!("mock-log-{}", state.log_id_counter),
            })
        }
    }

    fn upload_hash(
        &self,
        hash_base_url: Option<&str>,
        device_access_token: &str,
        content_hash: &[u8; 32],
    ) -> CoreResult<()> {
        let mut state = self.state();
        state.hash_uploads.push(HashCall {
            hash_base_url: hash_base_url.map(String::from),
            device_access_token: device_access_token.to_string(),
            content_hash: *content_hash,
        });
        if let Some(canned) = state.hash_responses.pop_front() {
            canned
        } else {
            Ok(())
        }
    }
}
