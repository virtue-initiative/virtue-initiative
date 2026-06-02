use std::any::Any;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};
use crate::model::UserSessionState;

#[derive(Debug)]
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
    UserSessionChanged,
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
        #[serde(skip_serializing_if = "Option::is_none")]
        session_state: Option<UserSessionState>,
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

#[derive(Debug)]
pub enum Event {
    Ping,
    ProcessStarted,
    Upload { risk: f32, kind: UploadKind },
    ProcessStopped(ProcessStoppedReason),
    ComputerSuspended,
    ComputerResumed,
    UserSessionChanged(UserSessionState),
    CaptureFailed,
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

    pub fn queue_event(&mut self, event: Event) {
        self.tx.send(event).ok();
    }

    pub fn iter(&mut self) -> CoreResult<()> {
        self.tx.send(Event::Ping).ok();
        while let Ok(event) = self.rx.try_recv() {
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
        serde_json::to_writer(file, &state)?;
        std::fs::rename(&tmp, &self.state_file_path)?;
        Ok(())
    }
}
