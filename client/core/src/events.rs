use std::any::Any;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};
use crate::model::{DeviceCredentials, DeviceSettings, ServiceStatus};

/// Wraps a value so that its `Debug` output is always `[REDACTED]`.
#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Redacted<T>(pub T);

impl<T> std::fmt::Debug for Redacted<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[REDACTED]")
    }
}

impl<T: std::ops::Deref<Target = str>> std::ops::Deref for Redacted<T> {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum ProcessStoppedReason {
    Other,
    Shutdown,
    User,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleKind {
    ProcessStarted,
    ProcessStoppedUser,
    ProcessStoppedShutdown,
    ProcessStoppedOther,
    ComputerSuspended,
    ComputerResumed,
    Login,
    Logout,
    ComputerBooted,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum AlertReason {
    ProcessKilledBeforeShutdown,
    UserStoppedProcess,
    UnexpectedProcessStart,
    PingGapWhileRunning,
    MissingResume,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum UploadKind {
    Screenshot {
        image: Vec<u8>,
        content_type: String,
    },
    Lifecycle {
        kind: LifecycleKind,
    },
    LifecycleAlert {
        reason: AlertReason,
    },
    Alert {
        message: String,
    },
    CaptureFailed,
    Dev {
        title: String,
        details: Option<String>,
    },
}

/// A single piece of `ServiceStatus` reported by one observer in response to a
/// `StatusRequest`. Each observer emits only the fields it owns; the
/// `StatusObserver` merges them into a complete `ServiceStatus`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum PartialStatus {
    /// Reported by `AuthObserver`.
    Auth {
        is_authenticated: bool,
        device_id: Option<String>,
    },
    /// Reported by `LifecycleObserver`.
    Lifecycle {
        is_running: bool,
        last_loop_at_ms: Option<i64>,
    },
    /// Reported by `UploadObserver`.
    Upload { pending_request_count: usize },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Event {
    Ping,
    ProcessStarted,
    Upload {
        risk: f32,
        kind: UploadKind,
    },
    ProcessStopped(ProcessStoppedReason),
    ComputerSuspended,
    ComputerResumed,
    // OS-level user session events — IPC inbound allowed
    UserSessionLogin,
    UserSessionLogout,
    // Service authentication events — NOT forwarded over IPC inbound
    Login {
        credentials: DeviceCredentials,
        settings: DeviceSettings,
    },
    Logout,
    DeviceSettingsRefreshed {
        settings: DeviceSettings,
    },
    CaptureFailed,
    // ── IPC status query ──────────────────────────────────────────────────
    StatusRequest,
    /// Emitted by an observer in response to `StatusRequest`; collected by the
    /// `StatusObserver` to assemble the `StatusResponse`.
    PartialStatus(PartialStatus),
    StatusResponse {
        status: ServiceStatus,
    },
    // ── controller → daemon requests ──────────────────────────────────────
    LoginRequested {
        email: String,
        password: Redacted<String>,
    },
    LogoutRequested,
    UserStopRequested {
        source: String,
    },
    // ── daemon → controller responses ─────────────────────────────────────
    LoginResult {
        success: bool,
        error: Option<String>,
        device_id: Option<String>,
    },
    LogoutResult {
        success: bool,
        error: Option<String>,
    },
}

pub type StateType = serde_json::Value;

pub trait Observer: Any {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn on_event(&mut self, event: &Event) -> CoreResult<()>;
    fn save_state(&self) -> CoreResult<StateType>;
    fn load_state(&mut self, state: StateType) -> CoreResult<()>;
    fn name(&self) -> &'static str;
}

pub fn log_error(msg: &str, err: Option<&dyn std::fmt::Display>) {
    match err {
        Some(e) => eprintln!("[core error] {msg}: {e}"),
        None => eprintln!("[core error] {msg}"),
    }
}

// ─── EventLoop ───────────────────────────────────────────────────────────────

pub struct EventLoop {
    pub observers: Vec<Box<dyn Observer>>,
    pub tx: Sender<Event>,
    rx: Receiver<Event>,
    pub(crate) state_file_path: PathBuf,
}

impl EventLoop {
    pub fn new(state_file_path: PathBuf, observers: Vec<Box<dyn Observer>>) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            observers,
            tx,
            rx,
            state_file_path,
        }
    }

    pub fn load_state(&mut self, path: &Path) -> CoreResult<()> {
        if path.exists() {
            let bytes = std::fs::read(path)?;
            let state: serde_json::Value = serde_json::from_slice(&bytes)?;
            if state.is_object() {
                let map = state.as_object().unwrap();
                for observer in &mut self.observers {
                    if let Some(observer_state) = map.get(observer.name()) {
                        observer.load_state(observer_state.clone())?;
                    }
                }
            } else {
                return Err(CoreError::InvalidState("State file is not a JSON object"));
            }
        }
        Ok(())
    }

    pub fn observer_mut<T: Observer + 'static>(&mut self) -> &mut T {
        self.observers
            .iter_mut()
            .find_map(|o| o.as_any_mut().downcast_mut::<T>())
            .expect("observer of requested type not found in event loop")
    }

    pub fn queue_event(&mut self, event: Event) {
        self.tx.send(event).ok();
    }

    /// Drains queued events through all observers without sending a Ping first.
    /// Used to process inbound IPC requests without triggering screenshot capture.
    pub fn drain_for_ipc(&mut self) -> CoreResult<()> {
        while let Ok(event) = self.rx.try_recv() {
            #[cfg(debug_assertions)]
            eprintln!("[core ipc event] {event:?}");
            for observer in &mut self.observers {
                observer.on_event(&event)?;
            }
        }
        Ok(())
    }

    pub fn iter(&mut self) -> CoreResult<()> {
        self.tx.send(Event::Ping).ok();
        while let Ok(event) = self.rx.try_recv() {
            #[cfg(debug_assertions)]
            eprintln!("[core event] {event:?}");
            for observer in &mut self.observers {
                observer.on_event(&event)?;
            }
        }
        self.persist()
    }

    pub fn persist(&self) -> CoreResult<()> {
        let mut state_map = serde_json::Map::new();
        for observer in &self.observers {
            state_map.insert(observer.name().to_string(), observer.save_state()?);
        }
        let state = serde_json::Value::Object(state_map);
        let tmp = self.state_file_path.with_extension("tmp");
        let file = File::create(&tmp)?;
        if let Err(e) = serde_json::to_writer(file, &state) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.into());
        }
        std::fs::rename(&tmp, &self.state_file_path)?;
        Ok(())
    }
}
