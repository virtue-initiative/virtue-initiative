pub mod api;
pub mod audit;
pub mod batch;
pub mod build_info;
pub mod config;
pub mod crypto;
pub mod error;
pub mod image_pipeline;
pub mod lifecycle;
pub mod model;
pub mod platform;
pub mod service;
pub mod storage;

pub use build_info::{BUILD_LABEL, build_label};
pub use config::Config;
pub use error::{CoreError, CoreResult};
pub use lifecycle::{
    CaptureAvailabilityState, CapturePermissionState, ComputerPowerState, LifecycleCapabilities,
    LifecycleCapabilitySupport, LifecycleConfidence, LifecycleDomain, LifecycleObservation,
    LifecycleOrigin, LifecycleSnapshot, LifecycleStatus, LifecycleTransition, ServicePingLog,
    ServiceRole, ServiceRuntimeState, ServiceStopMarker, StopIntent, UserSessionState,
};
pub use model::{
    AuditLogPayload, AuditRecord, AuditState, AuthState, BatchEvent, BatchEventData, BatchUpload,
    BufferedBatchEvent, DeviceCredentials, DeviceSettings, EventData, LogEntry, LoginStatus,
    LoopOutcome, Screenshot, ServiceStatus,
};
pub use platform::PlatformHooks;
pub use service::MonitorService;
