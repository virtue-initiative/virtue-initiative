use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::api::{ApiTransport, UploadedBatchResponse};
use crate::error::CoreResult;
use crate::model::{BatchUpload, DeviceCredentials, DeviceSettings, NotifyPayload};

/// Mock `ApiTransport` impl that records every call and serves either a
/// programmed canned response or a sensible default success.
///
/// Cheap to clone — the underlying state is `Arc<Mutex<_>>`, so handing a
/// clone to the test and moving another into the bus is the intended usage:
///
/// ```ignore
/// let mock = MockApiClient::new();
/// let inspector = mock.clone();
/// let observers = build_default_modules(cfg, platform, mock, PlatformConfig::default())?;
/// let mut bus = EventBus::new(observers, StateType::Null)?;
/// // ... drive the bus ...
/// assert_eq!(inspector.state().batch_uploads.len(), 2);
/// ```
#[derive(Clone)]
pub struct MockApiClient {
    state: Arc<Mutex<MockApiState>>,
}

pub struct MockApiState {
    // --- recordings ---
    pub login_calls: Vec<(String, String)>,
    pub logout_calls: Vec<()>,
    pub register_device_calls: Vec<RegisterDeviceCall>,
    pub get_device_settings_calls: Vec<String>,
    pub get_hash_token_calls: Vec<String>,
    pub batch_uploads: Vec<BatchCall>,
    pub notify_calls: Vec<NotifyCall>,
    pub hash_uploads: Vec<HashCall>,
    pub reconfigure_calls: Vec<String>,

    // --- canned responses (FIFO per method) ---
    pub login_responses: VecDeque<CoreResult<String>>,
    pub logout_responses: VecDeque<CoreResult<()>>,
    pub register_device_responses: VecDeque<CoreResult<DeviceCredentials>>,
    pub get_device_settings_responses: VecDeque<CoreResult<DeviceSettings>>,
    pub get_hash_token_responses: VecDeque<CoreResult<String>>,
    pub batch_responses: VecDeque<CoreResult<UploadedBatchResponse>>,
    pub notify_responses: VecDeque<CoreResult<()>>,
    pub hash_responses: VecDeque<CoreResult<()>>,

    // --- default values used when the canned queue is empty ---
    pub default_device_id: String,
    pub default_refresh_token: String,
    pub default_hash_token: String,
    pub default_device_settings: DeviceSettings,
    batch_id_counter: u64,
}

#[derive(Debug, Clone)]
pub struct RegisterDeviceCall {
    pub user_refresh_token: String,
    pub name: String,
    pub platform: String,
}

#[derive(Debug, Clone)]
pub struct BatchCall {
    pub device_refresh_token: String,
    pub batch: BatchUpload,
}

#[derive(Debug, Clone)]
pub struct NotifyCall {
    pub device_refresh_token: String,
    pub payload: NotifyPayload,
}

#[derive(Debug, Clone)]
pub struct HashCall {
    pub hash_base_url: Option<String>,
    pub hash_jwt: String,
    pub content_hash: [u8; 32],
}

impl Default for MockApiState {
    fn default() -> Self {
        Self {
            login_calls: Vec::new(),
            logout_calls: Vec::new(),
            register_device_calls: Vec::new(),
            get_device_settings_calls: Vec::new(),
            get_hash_token_calls: Vec::new(),
            batch_uploads: Vec::new(),
            notify_calls: Vec::new(),
            hash_uploads: Vec::new(),
            reconfigure_calls: Vec::new(),

            login_responses: VecDeque::new(),
            logout_responses: VecDeque::new(),
            register_device_responses: VecDeque::new(),
            get_device_settings_responses: VecDeque::new(),
            get_hash_token_responses: VecDeque::new(),
            batch_responses: VecDeque::new(),
            notify_responses: VecDeque::new(),
            hash_responses: VecDeque::new(),

            default_device_id: "mock-device".to_string(),
            default_refresh_token: "mock-refresh-token".to_string(),
            default_hash_token: "mock-hash-token".to_string(),
            default_device_settings: DeviceSettings {
                device_id: "mock-device".to_string(),
                name: "mock device".to_string(),
                platform: "mock".to_string(),
                wrapping_keys: vec![crate::model::BatchRecipient {
                    user_id: "mock-user".to_string(),
                    // X25519 base point (u=9); any valid curve point works here.
                    pub_key_base64: "CQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_string(),
                }],
                hash_base_url: None,
            },
            batch_id_counter: 0,
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

    pub fn program_get_hash_token(&self, response: CoreResult<String>) {
        self.state().get_hash_token_responses.push_back(response);
    }

    pub fn program_batch(&self, response: CoreResult<UploadedBatchResponse>) {
        self.state().batch_responses.push_back(response);
    }

    pub fn program_notify(&self, response: CoreResult<()>) {
        self.state().notify_responses.push_back(response);
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
    fn reconfigure(&mut self, api_base_url: &str) -> CoreResult<()> {
        self.state()
            .reconfigure_calls
            .push(api_base_url.to_string());
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
            Ok("mock-user-refresh-token".to_string())
        }
    }

    fn logout(&self) -> CoreResult<()> {
        let mut state = self.state();
        state.logout_calls.push(());
        if let Some(canned) = state.logout_responses.pop_front() {
            canned
        } else {
            Ok(())
        }
    }

    fn register_device(
        &self,
        user_refresh_token: &str,
        name: &str,
        platform: &str,
    ) -> CoreResult<DeviceCredentials> {
        let mut state = self.state();
        state.register_device_calls.push(RegisterDeviceCall {
            user_refresh_token: user_refresh_token.to_string(),
            name: name.to_string(),
            platform: platform.to_string(),
        });
        if let Some(canned) = state.register_device_responses.pop_front() {
            canned
        } else {
            Ok(DeviceCredentials {
                device_id: state.default_device_id.clone(),
                refresh_token: state.default_refresh_token.clone(),
            })
        }
    }

    fn get_device_settings(&self, device_refresh_token: &str) -> CoreResult<DeviceSettings> {
        let mut state = self.state();
        state
            .get_device_settings_calls
            .push(device_refresh_token.to_string());
        if let Some(canned) = state.get_device_settings_responses.pop_front() {
            canned
        } else {
            Ok(state.default_device_settings.clone())
        }
    }

    fn get_hash_token(&self, device_refresh_token: &str) -> CoreResult<String> {
        let mut state = self.state();
        state
            .get_hash_token_calls
            .push(device_refresh_token.to_string());
        if let Some(canned) = state.get_hash_token_responses.pop_front() {
            canned
        } else {
            Ok(state.default_hash_token.clone())
        }
    }

    fn upload_batch(
        &self,
        device_refresh_token: &str,
        batch: &BatchUpload,
    ) -> CoreResult<UploadedBatchResponse> {
        let mut state = self.state();
        state.batch_uploads.push(BatchCall {
            device_refresh_token: device_refresh_token.to_string(),
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

    fn notify(&self, device_refresh_token: &str, payload: &NotifyPayload) -> CoreResult<()> {
        let mut state = self.state();
        state.notify_calls.push(NotifyCall {
            device_refresh_token: device_refresh_token.to_string(),
            payload: payload.clone(),
        });
        if let Some(canned) = state.notify_responses.pop_front() {
            canned
        } else {
            Ok(())
        }
    }

    fn upload_hash(
        &self,
        hash_base_url: Option<&str>,
        hash_jwt: &str,
        content_hash: &[u8; 32],
    ) -> CoreResult<()> {
        let mut state = self.state();
        state.hash_uploads.push(HashCall {
            hash_base_url: hash_base_url.map(String::from),
            hash_jwt: hash_jwt.to_string(),
            content_hash: *content_hash,
        });
        if let Some(canned) = state.hash_responses.pop_front() {
            canned
        } else {
            Ok(())
        }
    }
}
