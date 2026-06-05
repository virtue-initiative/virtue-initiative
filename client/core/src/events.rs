use std::any::Any;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};
use crate::model::{DeviceCredentials, DeviceSettings, ServiceStatus};

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

#[derive(Serialize, Deserialize)]
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
    StatusResponse {
        status: ServiceStatus,
    },
    // ── controller → daemon requests ──────────────────────────────────────
    LoginRequested {
        email: String,
        password: String,
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

impl std::fmt::Debug for Event {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Event::Ping => write!(f, "Ping"),
            Event::ProcessStarted => write!(f, "ProcessStarted"),
            Event::Upload { risk, kind } => f
                .debug_struct("Upload")
                .field("risk", risk)
                .field("kind", kind)
                .finish(),
            Event::ProcessStopped(r) => write!(f, "ProcessStopped({r:?})"),
            Event::ComputerSuspended => write!(f, "ComputerSuspended"),
            Event::ComputerResumed => write!(f, "ComputerResumed"),
            Event::UserSessionLogin => write!(f, "UserSessionLogin"),
            Event::UserSessionLogout => write!(f, "UserSessionLogout"),
            Event::Login { credentials, .. } => f
                .debug_struct("Login")
                .field("device_id", &credentials.device_id)
                .finish(),
            Event::Logout => write!(f, "Logout"),
            Event::DeviceSettingsRefreshed { .. } => write!(f, "DeviceSettingsRefreshed"),
            Event::CaptureFailed => write!(f, "CaptureFailed"),
            Event::StatusRequest => write!(f, "StatusRequest"),
            Event::StatusResponse { status } => f
                .debug_struct("StatusResponse")
                .field("status", status)
                .finish(),
            Event::LoginRequested { email, .. } => f
                .debug_struct("LoginRequested")
                .field("email", email)
                .field("password", &"[REDACTED]")
                .finish(),
            Event::LogoutRequested => write!(f, "LogoutRequested"),
            Event::UserStopRequested { source } => f
                .debug_struct("UserStopRequested")
                .field("source", source)
                .finish(),
            Event::LoginResult {
                success,
                error,
                device_id,
            } => f
                .debug_struct("LoginResult")
                .field("success", success)
                .field("error", error)
                .field("device_id", device_id)
                .finish(),
            Event::LogoutResult { success, error } => f
                .debug_struct("LogoutResult")
                .field("success", success)
                .field("error", error)
                .finish(),
        }
    }
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
