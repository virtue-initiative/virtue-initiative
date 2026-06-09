pub mod api;
pub mod build_info;
pub mod config;
pub mod controller;
pub mod crypto;
pub mod error;
pub mod events;
pub mod ipc;
pub mod model;
pub mod platform;
pub mod state;
pub mod storage;

// Temporarily excluded: depend on the old Event<C> system; will be migrated in plan 02.
#[cfg(any())]
pub mod module;
#[cfg(any())]
pub mod service;
#[cfg(any())]
#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use build_info::{BUILD_LABEL, build_label};
pub use config::Config;
pub use controller::ClientController;
pub use error::{CoreError, CoreResult};
pub use events::{
    // Payload re-exports
    AlertReason,
    CaptureFailed,
    ComputerResumed,
    ComputerSuspended,
    ConfigChanged,
    DeviceCredentials,
    DeviceSettings,
    DeviceSettingsRefreshed,
    Emitter,
    Error as EventError,
    Event,
    EventBus,
    EventChannel,
    FlushBatchNow,
    LifecycleKind,
    Login,
    LoginRequested,
    LoginResult,
    Logout,
    LogoutRequested,
    LogoutResult,
    Observer,
    PartialStatus,
    Ping,
    ProcessStarted,
    ProcessStopped,
    ProcessStoppedReason,
    Redacted,
    RemoteEventBus,
    RemoteSender,
    StateType,
    StatusRequest,
    StatusResponse,
    Upload,
    UploadKind,
    UserSessionLogin,
    UserSessionLogout,
    UserStopRequested,
};
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub use ipc::register_connect_tx;
pub use ipc::{IpcError, IpcListener};
pub use model::{
    AuthState, BatchUpload, EventData, LogEntry, LoginStatus, LoopOutcome, Screenshot,
    ServiceStatus,
};
pub use platform::{PlatformHooks, ScreenshotHooks};
pub use state::{load_state, store_state};
