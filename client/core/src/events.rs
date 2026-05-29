mod lifecycle;
pub(crate) mod screenshot;
mod upload;

pub(crate) use lifecycle::{LifecycleObserver, LifecycleObserverState};
pub(crate) use screenshot::{ScreenshotConfig, ScreenshotObserver, ScreenshotObserverState};
pub(crate) use upload::{UploadConfig, UploadObserver, UploadObserverState};

use std::collections::VecDeque;
use std::fs::File;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::api::ApiTransport;
use crate::error::{CoreError, CoreResult};
use crate::lifecycle::LifecycleObservation;
use crate::model::{BatchLogEntry, LogEntry};
use crate::platform::PlatformHooks;

pub(crate) const POST_LOGIN_PROOF_BATCH_COUNT: u32 = 3;
pub(crate) const MAX_BATCH_ITEMS_PER_UPLOAD: usize = 200;
pub(crate) const HIGH_RISK_LIFECYCLE_ALERT: f32 = 0.9;
pub(crate) const SERVICE_PING_INTERVAL_MS: i64 = 60_000;
pub(crate) const SERVICE_PING_GRACE_MS: i64 = 10_000;

pub enum Event {
    Tick {
        now_ms: i64,
    },
    ScreenshotCaptured {
        data: BatchLogEntry,
    },
    ImmediateUpload {
        entry: LogEntry,
    },
    BatchUpload {
        data: BatchLogEntry,
    },
    Shutdown,
    /// A platform-supplied lifecycle observation to be folded into lifecycle
    /// state. `is_authenticated` is captured by the service before queuing.
    LifecycleObserved {
        observation: LifecycleObservation,
        now_ms: i64,
        is_authenticated: bool,
    },
}

// ─── Serializable observer state ─────────────────────────────────────────────

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct ObserversState {
    #[serde(default)]
    pub screenshot: ScreenshotObserverState,
    #[serde(default)]
    pub upload: UploadObserverState,
    #[serde(default)]
    pub lifecycle: LifecycleObserverState,
}

// ─── Runtime observers ───────────────────────────────────────────────────────

pub struct Observers<P: PlatformHooks, A: ApiTransport + Clone> {
    pub screenshot: ScreenshotObserver<P>,
    pub upload: UploadObserver<A>,
    pub lifecycle: LifecycleObserver,
}

impl<P: PlatformHooks, A: ApiTransport + Clone> Observers<P, A> {
    pub(crate) fn on_event(&mut self, event: &Event, now_ms: i64) -> CoreResult<Vec<Event>> {
        let mut new_events = Vec::new();
        new_events.extend(self.screenshot.on_event(event, now_ms)?);
        new_events.extend(self.upload.on_event(event, now_ms)?);
        new_events.extend(self.lifecycle.on_event(event, now_ms)?);
        Ok(new_events)
    }
}

// ─── EventLoop ───────────────────────────────────────────────────────────────

pub struct EventLoop<P: PlatformHooks, A: ApiTransport + Clone> {
    pub observers: Observers<P, A>,
    pending_events: VecDeque<Event>,
    pub(crate) state_file_path: PathBuf,
}

impl<P: PlatformHooks, A: ApiTransport + Clone> EventLoop<P, A> {
    pub fn load_state(path: &Path) -> CoreResult<ObserversState> {
        if path.exists() {
            let bytes = std::fs::read(path)?;
            Ok(serde_json::from_slice(&bytes).unwrap_or_default())
        } else {
            Ok(ObserversState::default())
        }
    }

    pub fn new(state_file_path: PathBuf, observers: Observers<P, A>) -> Self {
        Self {
            observers,
            pending_events: VecDeque::new(),
            state_file_path,
        }
    }

    pub fn queue_event(&mut self, event: Event) {
        self.pending_events.push_back(event);
    }

    pub fn iter(&mut self, now_ms: i64) -> CoreResult<()> {
        let mut pending = std::mem::take(&mut self.pending_events);
        pending.push_back(Event::Tick { now_ms });
        while let Some(event) = pending.pop_front() {
            let new_events = self.observers.on_event(&event, now_ms)?;
            pending.extend(new_events);
        }
        self.persist()
    }

    /// Process a single event (and any events it cascades) without injecting a
    /// `Tick`, then persist. Used for state-only updates that should not
    /// trigger captures or uploads.
    pub fn dispatch(&mut self, event: Event, now_ms: i64) -> CoreResult<()> {
        let mut pending = VecDeque::new();
        pending.push_back(event);
        while let Some(event) = pending.pop_front() {
            let new_events = self.observers.on_event(&event, now_ms)?;
            pending.extend(new_events);
        }
        self.persist()
    }

    pub fn persist(&self) -> CoreResult<()> {
        let state = ObserversState {
            screenshot: self.observers.screenshot.state.clone(),
            upload: self.observers.upload.state.clone(),
            lifecycle: self.observers.lifecycle.state.clone(),
        };
        let tmp = self.state_file_path.with_extension("tmp");
        let file = File::create(&tmp)?;
        serde_json::to_writer(file, &state)?;
        std::fs::rename(&tmp, &self.state_file_path)?;
        Ok(())
    }
}

// ─── Shared helpers ──────────────────────────────────────────────────────────

pub(crate) fn log_error(message: &str, error: Option<&CoreError>) {
    let error_text = error
        .map(ToString::to_string)
        .unwrap_or_else(|| "unknown error".to_string());
    eprintln!("[event] {message}; error={error_text}");
}
