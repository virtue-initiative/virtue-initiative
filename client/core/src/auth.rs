use crate::error::CoreResult;
use crate::model::{AuthState, DeviceCredentials};
use crate::storage::FileStateStore;

pub struct Auth {
    pub user_access_token: Option<String>,
    pub device_credentials: Option<DeviceCredentials>,
}

impl Auth {
    pub fn load(storage: &FileStateStore) -> CoreResult<Self> {
        let state = storage.load_auth_state()?;
        Ok(Self {
            user_access_token: state.user_access_token,
            device_credentials: state.device_credentials,
        })
    }

    pub fn persist(&self, storage: &FileStateStore) -> CoreResult<()> {
        storage.save_auth_state(&AuthState {
            user_access_token: self.user_access_token.clone(),
            device_credentials: self.device_credentials.clone(),
        })
    }

    pub fn is_authenticated(&self) -> bool {
        self.device_credentials.is_some()
    }

    pub fn device_id(&self) -> Option<&str> {
        self.device_credentials
            .as_ref()
            .map(|c| c.device_id.as_str())
    }

    pub fn set_login(&mut self, token: String, creds: DeviceCredentials) {
        self.user_access_token = Some(token);
        self.device_credentials = Some(creds);
    }

    pub fn set_credentials(&mut self, creds: DeviceCredentials) {
        self.device_credentials = Some(creds);
    }

    pub fn clear(&mut self) {
        self.user_access_token = None;
        self.device_credentials = None;
    }
}
