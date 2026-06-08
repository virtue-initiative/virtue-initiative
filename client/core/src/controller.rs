use std::path::Path;

use serde::de::DeserializeOwned;

use crate::events::{Event, ProcessStoppedReason, Redacted};
use crate::ipc::{self, IpcError, IpcReceiver, IpcSender};
use crate::model::ServiceStatus;

/// High-level client for communicating with a running daemon over IPC.
/// Mirrors the `MonitorService` API but routes requests through the daemon
/// instead of executing them in-process.
pub struct ControllerClient {
    sender: IpcSender,
    receiver: IpcReceiver,
}

impl ControllerClient {
    pub fn connect(path: &Path) -> Result<Self, IpcError> {
        let (sender, receiver) = ipc::connect(path)?;
        Ok(Self { sender, receiver })
    }

    /// Send `LoginRequested` and block until `LoginResult` is received.
    /// Returns the new device ID on success.
    pub fn login(&mut self, email: &str, password: &str) -> Result<String, IpcError> {
        self.sender.send::<serde_json::Value>(&Event::LoginRequested {
            email: email.to_string(),
            password: Redacted(password.to_string()),
        })?;
        loop {
            match self.recv_event_internal()? {
                Event::LoginResult {
                    success: true,
                    device_id,
                    ..
                } => return Ok(device_id.unwrap_or_default()),
                Event::LoginResult {
                    success: false,
                    error,
                    ..
                } => {
                    return Err(IpcError::Remote(
                        error.unwrap_or_else(|| "login failed".to_string()),
                    ));
                }
                _ => {}
            }
        }
    }

    /// Send `LogoutRequested` and block until `LogoutResult` is received.
    pub fn logout(&mut self) -> Result<(), IpcError> {
        self.sender.send::<serde_json::Value>(&Event::LogoutRequested)?;
        loop {
            match self.recv_event_internal()? {
                Event::LogoutResult { success: true, .. } => return Ok(()),
                Event::LogoutResult {
                    success: false,
                    error,
                } => {
                    return Err(IpcError::Remote(
                        error.unwrap_or_else(|| "logout failed".to_string()),
                    ));
                }
                _ => {}
            }
        }
    }

    /// Send `StatusRequest` and block until `StatusResponse` is received.
    pub fn get_status(&mut self) -> Result<ServiceStatus, IpcError> {
        self.sender.send::<serde_json::Value>(&Event::StatusRequest)?;
        loop {
            if let Event::StatusResponse { status } = self.recv_event_internal()? {
                return Ok(status);
            }
        }
    }

    /// Send `StatusRequest`, block until `StatusResponse`, and call `handler`
    /// for each `Custom` event received while waiting.
    pub fn get_status_with_handler<C, F>(&mut self, mut handler: F) -> Result<ServiceStatus, IpcError>
    where
        C: DeserializeOwned,
        F: FnMut(C),
    {
        self.sender.send::<serde_json::Value>(&Event::StatusRequest)?;
        loop {
            match self.recv_event::<C>()? {
                Event::StatusResponse { status } => return Ok(status),
                Event::Custom(c) => handler(c),
                _ => {}
            }
        }
    }

    /// Fire-and-forget: ask the daemon to record a user-initiated stop.
    pub fn request_user_stop(&mut self, source: &str) -> Result<(), IpcError> {
        self.sender.send::<serde_json::Value>(&Event::UserStopRequested {
            source: source.to_string(),
        })
    }

    pub fn note_suspended(&mut self) -> Result<(), IpcError> {
        self.sender.send::<serde_json::Value>(&Event::ComputerSuspended)
    }

    pub fn note_resumed(&mut self) -> Result<(), IpcError> {
        self.sender.send::<serde_json::Value>(&Event::ComputerResumed)
    }

    /// Send the OS-level user session login event (e.g. Windows logon).
    pub fn note_login(&mut self) -> Result<(), IpcError> {
        self.sender.send::<serde_json::Value>(&Event::UserSessionLogin)
    }

    /// Send the OS-level user session logout event (e.g. Windows logoff).
    pub fn note_logout(&mut self) -> Result<(), IpcError> {
        self.sender.send::<serde_json::Value>(&Event::UserSessionLogout)
    }

    pub fn note_process_stopped(&mut self, reason: ProcessStoppedReason) -> Result<(), IpcError> {
        self.sender.send::<serde_json::Value>(&Event::ProcessStopped(reason))
    }

    /// Receive an event with a custom payload type `C`.
    pub fn recv_event<C: DeserializeOwned>(&mut self) -> Result<Event<C>, IpcError> {
        self.receiver.recv_event::<C>()
    }

    /// Non-blocking receive with a custom payload type `C`.
    pub fn try_recv_event<C: DeserializeOwned>(&mut self) -> Result<Option<Event<C>>, IpcError> {
        self.receiver.try_recv_event::<C>()
    }

    /// Internal helper that uses `serde_json::Value` as the custom event type,
    /// tolerating any daemon-side custom variant without needing the concrete type.
    fn recv_event_internal(&mut self) -> Result<Event<serde_json::Value>, IpcError> {
        self.receiver.recv_event::<serde_json::Value>()
    }
}
