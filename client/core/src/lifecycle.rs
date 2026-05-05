use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ComputerPowerState {
    Started,
    Running,
    Suspending,
    Suspended,
    Waking,
    ShuttingDown,
    #[default]
    Unknown,
}

impl ComputerPowerState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Running => "running",
            Self::Suspending => "suspending",
            Self::Suspended => "suspended",
            Self::Waking => "waking",
            Self::ShuttingDown => "shutting_down",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum UserSessionState {
    LoggedIn,
    LoggedOut,
    #[default]
    Unknown,
}

impl UserSessionState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LoggedIn => "logged_in",
            Self::LoggedOut => "logged_out",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ServiceRuntimeState {
    Running,
    Stopping,
    Stopped,
    Crashed,
    #[default]
    Unknown,
}

impl ServiceRuntimeState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Stopping => "stopping",
            Self::Stopped => "stopped",
            Self::Crashed => "crashed",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CapturePermissionState {
    Granted,
    Missing,
    Unsupported,
    #[default]
    Unknown,
}

impl CapturePermissionState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Granted => "granted",
            Self::Missing => "missing",
            Self::Unsupported => "unsupported",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CaptureAvailabilityState {
    Ready,
    Blocked,
    Unsupported,
    #[default]
    Unknown,
}

impl CaptureAvailabilityState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Blocked => "blocked",
            Self::Unsupported => "unsupported",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ServiceRole {
    PrimaryService,
    CaptureWorker,
}

impl ServiceRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PrimaryService => "primary_service",
            Self::CaptureWorker => "capture_worker",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleOrigin {
    UserRequested,
    SystemShutdown,
    SystemSuspend,
    SessionLogout,
    ServiceManager,
    CrashOrKill,
    StartupRecovery,
    Unknown,
}

impl LifecycleOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserRequested => "user_requested",
            Self::SystemShutdown => "system_shutdown",
            Self::SystemSuspend => "system_suspend",
            Self::SessionLogout => "session_logout",
            Self::ServiceManager => "service_manager",
            Self::CrashOrKill => "crash_or_kill",
            Self::StartupRecovery => "startup_recovery",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleConfidence {
    Confirmed,
    BestEffort,
    Inferred,
}

impl LifecycleConfidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::BestEffort => "best_effort",
            Self::Inferred => "inferred",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleCapabilitySupport {
    /// The platform has an intended implementation for this lifecycle signal and
    /// should emit it when the underlying OS/app event occurs.
    Supported,
    /// The platform can sometimes infer or observe this lifecycle signal, but
    /// detection is incomplete or not reliable in every real-world case.
    BestEffort,
    /// The platform does not currently track this lifecycle signal and callers
    /// should treat missing transitions as expected rather than suspicious.
    #[default]
    Unsupported,
}

impl LifecycleCapabilitySupport {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::BestEffort => "best_effort",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct LifecycleCapabilities {
    /// Detect whether the current runtime instance started on a new boot, such as
    /// by comparing a boot identifier or equivalent OS marker.
    pub startup: LifecycleCapabilitySupport,
    /// Detect a service stop that is attributable to host shutdown or reboot
    /// rather than an arbitrary external termination.
    pub shutdown: LifecycleCapabilitySupport,
    /// Detect the machine entering suspend/sleep.
    pub suspend: LifecycleCapabilitySupport,
    /// Detect the machine resuming from suspend/sleep.
    pub wake: LifecycleCapabilitySupport,
    /// Detect an OS-level user login/session start transition.
    pub user_login: LifecycleCapabilitySupport,
    /// Detect an OS-level user logout/session end transition.
    pub user_logout: LifecycleCapabilitySupport,
    /// Distinguish an explicit user-requested monitoring stop from a generic stop,
    /// typically by recording a local stop-intent marker before shutdown.
    pub explicit_user_stop: LifecycleCapabilitySupport,
    /// Detect whether the platform-specific permission required for capture is
    /// currently granted or missing.
    pub capture_permission: LifecycleCapabilitySupport,
    /// Detect whether capture is actually usable right now, even if the service is
    /// running, such as blocked session access or unavailable display resources.
    pub capture_availability: LifecycleCapabilitySupport,
    /// Track the additional Windows-only capture worker process separately from
    /// the primary monitoring service.
    pub capture_worker: LifecycleCapabilitySupport,
    /// Recover a missed stop/crash on the next startup by comparing persisted
    /// lifecycle state against a new boot marker.
    pub next_boot_recovery: LifecycleCapabilitySupport,
}

impl LifecycleCapabilities {
    pub fn for_platform(platform_name: &str) -> Self {
        match platform_name {
            "linux" => Self {
                startup: LifecycleCapabilitySupport::Supported,
                shutdown: LifecycleCapabilitySupport::BestEffort,
                suspend: LifecycleCapabilitySupport::Supported,
                wake: LifecycleCapabilitySupport::Supported,
                user_login: LifecycleCapabilitySupport::Unsupported,
                user_logout: LifecycleCapabilitySupport::Unsupported,
                explicit_user_stop: LifecycleCapabilitySupport::Supported,
                capture_permission: LifecycleCapabilitySupport::Unsupported,
                capture_availability: LifecycleCapabilitySupport::Supported,
                capture_worker: LifecycleCapabilitySupport::Unsupported,
                next_boot_recovery: LifecycleCapabilitySupport::Supported,
            },
            "macos" => Self {
                startup: LifecycleCapabilitySupport::Supported,
                shutdown: LifecycleCapabilitySupport::BestEffort,
                suspend: LifecycleCapabilitySupport::Supported,
                wake: LifecycleCapabilitySupport::Supported,
                user_login: LifecycleCapabilitySupport::Unsupported,
                user_logout: LifecycleCapabilitySupport::Unsupported,
                explicit_user_stop: LifecycleCapabilitySupport::Supported,
                capture_permission: LifecycleCapabilitySupport::Supported,
                capture_availability: LifecycleCapabilitySupport::BestEffort,
                capture_worker: LifecycleCapabilitySupport::Unsupported,
                next_boot_recovery: LifecycleCapabilitySupport::Supported,
            },
            "windows" => Self {
                startup: LifecycleCapabilitySupport::Supported,
                shutdown: LifecycleCapabilitySupport::Supported,
                suspend: LifecycleCapabilitySupport::Supported,
                wake: LifecycleCapabilitySupport::Supported,
                user_login: LifecycleCapabilitySupport::BestEffort,
                user_logout: LifecycleCapabilitySupport::Supported,
                explicit_user_stop: LifecycleCapabilitySupport::Supported,
                capture_permission: LifecycleCapabilitySupport::Unsupported,
                capture_availability: LifecycleCapabilitySupport::Supported,
                capture_worker: LifecycleCapabilitySupport::Unsupported,
                next_boot_recovery: LifecycleCapabilitySupport::Supported,
            },
            "android" => Self {
                startup: LifecycleCapabilitySupport::Unsupported,
                shutdown: LifecycleCapabilitySupport::Unsupported,
                suspend: LifecycleCapabilitySupport::Unsupported,
                wake: LifecycleCapabilitySupport::Unsupported,
                user_login: LifecycleCapabilitySupport::Unsupported,
                user_logout: LifecycleCapabilitySupport::Unsupported,
                explicit_user_stop: LifecycleCapabilitySupport::Unsupported,
                capture_permission: LifecycleCapabilitySupport::Supported,
                capture_availability: LifecycleCapabilitySupport::Supported,
                capture_worker: LifecycleCapabilitySupport::Unsupported,
                next_boot_recovery: LifecycleCapabilitySupport::Unsupported,
            },
            "ios" => Self {
                startup: LifecycleCapabilitySupport::Unsupported,
                shutdown: LifecycleCapabilitySupport::Unsupported,
                suspend: LifecycleCapabilitySupport::Unsupported,
                wake: LifecycleCapabilitySupport::Unsupported,
                user_login: LifecycleCapabilitySupport::Unsupported,
                user_logout: LifecycleCapabilitySupport::Unsupported,
                explicit_user_stop: LifecycleCapabilitySupport::Unsupported,
                capture_permission: LifecycleCapabilitySupport::Supported,
                capture_availability: LifecycleCapabilitySupport::Supported,
                capture_worker: LifecycleCapabilitySupport::Unsupported,
                next_boot_recovery: LifecycleCapabilitySupport::Unsupported,
            },
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct LifecycleSnapshot {
    pub computer_power: ComputerPowerState,
    pub user_session: UserSessionState,
    pub primary_service: ServiceRuntimeState,
    pub capture_worker: ServiceRuntimeState,
    pub capture_permission: CapturePermissionState,
    pub capture_availability: CaptureAvailabilityState,
}

impl LifecycleSnapshot {
    pub fn service_state(&self, role: ServiceRole) -> ServiceRuntimeState {
        match role {
            ServiceRole::PrimaryService => self.primary_service,
            ServiceRole::CaptureWorker => self.capture_worker,
        }
    }

    pub fn set_service_state(&mut self, role: ServiceRole, state: ServiceRuntimeState) {
        match role {
            ServiceRole::PrimaryService => self.primary_service = state,
            ServiceRole::CaptureWorker => self.capture_worker = state,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleDomain {
    ComputerPower,
    UserSession,
    PrimaryService,
    CaptureWorker,
    CapturePermission,
    CaptureAvailability,
}

impl LifecycleDomain {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ComputerPower => "computer_power",
            Self::UserSession => "user_session",
            Self::PrimaryService => "primary_service",
            Self::CaptureWorker => "capture_worker",
            Self::CapturePermission => "capture_permission",
            Self::CaptureAvailability => "capture_availability",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct LifecycleTransition {
    pub domain: LifecycleDomain,
    pub service_role: Option<ServiceRole>,
    pub from: String,
    pub to: String,
    pub origin: LifecycleOrigin,
    pub detected_by: String,
    pub confidence: LifecycleConfidence,
    pub risk: f32,
}

impl Default for LifecycleTransition {
    fn default() -> Self {
        Self {
            domain: LifecycleDomain::PrimaryService,
            service_role: None,
            from: "unknown".to_string(),
            to: "unknown".to_string(),
            origin: LifecycleOrigin::Unknown,
            detected_by: String::new(),
            confidence: LifecycleConfidence::Confirmed,
            risk: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct LifecycleStatus {
    pub snapshot: LifecycleSnapshot,
    pub last_transition: Option<LifecycleTransition>,
    pub last_stop_origin: Option<LifecycleOrigin>,
    pub last_emitted_risk: Option<f32>,
    pub last_boot_marker: Option<String>,
    pub capabilities: LifecycleCapabilities,
}

impl LifecycleStatus {
    pub fn for_platform(platform_name: &str) -> Self {
        Self {
            capabilities: LifecycleCapabilities::for_platform(platform_name),
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LifecycleObservation {
    BootObserved {
        boot_marker: String,
        booted_at_ms: Option<i64>,
        detected_by: String,
    },
    ComputerPowerChanged {
        state: ComputerPowerState,
        origin: LifecycleOrigin,
        detected_by: String,
        confidence: LifecycleConfidence,
    },
    StopRequestedByUser {
        role: ServiceRole,
        source: String,
    },
    ServiceStarted {
        role: ServiceRole,
        detected_by: String,
    },
    ServiceStopObserved {
        role: ServiceRole,
        raw_reason: String,
        shutdown_in_progress: bool,
        explicit_user_stop: bool,
        detected_by: String,
    },
    ProcessMissing {
        role: ServiceRole,
        had_expected_runtime: bool,
        detected_by: String,
    },
    UserSessionChanged {
        state: UserSessionState,
        origin: LifecycleOrigin,
        detected_by: String,
    },
    CapturePermissionChanged {
        state: CapturePermissionState,
        detected_by: String,
    },
    CaptureAvailabilityChanged {
        state: CaptureAvailabilityState,
        detected_by: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct StopIntent {
    pub role: ServiceRole,
    pub source: String,
    pub requested_at_ms: i64,
}

impl Default for StopIntent {
    fn default() -> Self {
        Self {
            role: ServiceRole::PrimaryService,
            source: String::new(),
            requested_at_ms: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ServiceStopMarker {
    pub role: ServiceRole,
    pub origin: LifecycleOrigin,
    pub stopped_at_ms: i64,
}

impl Default for ServiceStopMarker {
    fn default() -> Self {
        Self {
            role: ServiceRole::PrimaryService,
            origin: LifecycleOrigin::Unknown,
            stopped_at_ms: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ServicePingLog {
    pub role: ServiceRole,
    pub pinged_at_ms: i64,
    pub gap_ms: Option<i64>,
    pub risk: f32,
    pub detected_by: String,
}

impl Default for ServicePingLog {
    fn default() -> Self {
        Self {
            role: ServiceRole::PrimaryService,
            pinged_at_ms: 0,
            gap_ms: None,
            risk: 0.0,
            detected_by: String::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LifecycleReduceResult {
    pub status: LifecycleStatus,
    pub transitions: Vec<LifecycleTransition>,
}

struct TransitionInput<'a> {
    domain: LifecycleDomain,
    service_role: Option<ServiceRole>,
    from: &'a str,
    to: &'a str,
    origin: LifecycleOrigin,
    detected_by: &'a str,
    confidence: LifecycleConfidence,
    risk: f32,
}

fn new_transition(input: TransitionInput<'_>) -> LifecycleTransition {
    LifecycleTransition {
        domain: input.domain,
        service_role: input.service_role,
        from: input.from.to_string(),
        to: input.to.to_string(),
        origin: input.origin,
        detected_by: input.detected_by.to_string(),
        confidence: input.confidence,
        risk: input.risk,
    }
}

fn stop_origin_from_raw_reason(raw_reason: &str) -> LifecycleOrigin {
    let normalized = raw_reason.trim().to_ascii_lowercase();
    if normalized.contains("session_logout") || normalized.contains("logoff") {
        LifecycleOrigin::SessionLogout
    } else if normalized.contains("service_control_stop") {
        LifecycleOrigin::ServiceManager
    } else {
        LifecycleOrigin::Unknown
    }
}

fn stop_risk(origin: LifecycleOrigin) -> f32 {
    match origin {
        LifecycleOrigin::SystemShutdown | LifecycleOrigin::UserRequested => 0.0,
        LifecycleOrigin::Unknown
        | LifecycleOrigin::CrashOrKill
        | LifecycleOrigin::ServiceManager
        | LifecycleOrigin::StartupRecovery => 0.5,
        LifecycleOrigin::SystemSuspend | LifecycleOrigin::SessionLogout => 0.0,
    }
}

fn is_sleeping_power_state(state: ComputerPowerState) -> bool {
    matches!(
        state,
        ComputerPowerState::Suspending | ComputerPowerState::Suspended
    )
}

pub fn apply_observation(
    current: &LifecycleStatus,
    observation: &LifecycleObservation,
) -> LifecycleReduceResult {
    let mut status = current.clone();
    let mut transitions = Vec::new();

    match observation {
        LifecycleObservation::BootObserved {
            boot_marker,
            booted_at_ms: _,
            detected_by,
        } => {
            let boot_changed = status.last_boot_marker.as_deref() != Some(boot_marker.as_str());
            if boot_changed {
                for role in [ServiceRole::PrimaryService, ServiceRole::CaptureWorker] {
                    let state = status.snapshot.service_state(role);
                    if state == ServiceRuntimeState::Running {
                        let domain = match role {
                            ServiceRole::PrimaryService => LifecycleDomain::PrimaryService,
                            ServiceRole::CaptureWorker => LifecycleDomain::CaptureWorker,
                        };
                        status
                            .snapshot
                            .set_service_state(role, ServiceRuntimeState::Crashed);
                        let transition = new_transition(TransitionInput {
                            domain,
                            service_role: Some(role),
                            from: ServiceRuntimeState::Running.as_str(),
                            to: ServiceRuntimeState::Crashed.as_str(),
                            origin: LifecycleOrigin::StartupRecovery,
                            detected_by,
                            confidence: LifecycleConfidence::Inferred,
                            risk: stop_risk(LifecycleOrigin::StartupRecovery),
                        });
                        status.last_stop_origin = Some(LifecycleOrigin::StartupRecovery);
                        status.last_emitted_risk = Some(transition.risk);
                        status.last_transition = Some(transition.clone());
                        transitions.push(transition);
                    }
                }

                if status.snapshot.computer_power != ComputerPowerState::Started {
                    let transition = new_transition(TransitionInput {
                        domain: LifecycleDomain::ComputerPower,
                        service_role: None,
                        from: status.snapshot.computer_power.as_str(),
                        to: ComputerPowerState::Started.as_str(),
                        origin: LifecycleOrigin::Unknown,
                        detected_by,
                        confidence: LifecycleConfidence::Confirmed,
                        risk: 0.0,
                    });
                    status.snapshot.computer_power = ComputerPowerState::Started;
                    status.last_emitted_risk = Some(transition.risk);
                    status.last_transition = Some(transition.clone());
                    transitions.push(transition);
                }
                status.last_boot_marker = Some(boot_marker.clone());
            }
        }
        LifecycleObservation::ComputerPowerChanged {
            state,
            origin,
            detected_by,
            confidence,
        } => {
            if status.snapshot.computer_power != *state {
                let transition = new_transition(TransitionInput {
                    domain: LifecycleDomain::ComputerPower,
                    service_role: None,
                    from: status.snapshot.computer_power.as_str(),
                    to: state.as_str(),
                    origin: *origin,
                    detected_by,
                    confidence: *confidence,
                    risk: 0.0,
                });
                status.snapshot.computer_power = *state;
                status.last_emitted_risk = Some(transition.risk);
                status.last_transition = Some(transition.clone());
                transitions.push(transition);
            }
        }
        LifecycleObservation::StopRequestedByUser { .. } => {}
        LifecycleObservation::ServiceStarted { role, detected_by } => {
            let mut previous = status.snapshot.service_state(*role);
            if previous == ServiceRuntimeState::Running {
                previous = ServiceRuntimeState::Unknown;
            }
            status
                .snapshot
                .set_service_state(*role, ServiceRuntimeState::Running);
            if matches!(
                status.snapshot.computer_power,
                ComputerPowerState::Started | ComputerPowerState::Unknown
            ) {
                status.snapshot.computer_power = ComputerPowerState::Running;
            }
            let transition = new_transition(TransitionInput {
                domain: match role {
                    ServiceRole::PrimaryService => LifecycleDomain::PrimaryService,
                    ServiceRole::CaptureWorker => LifecycleDomain::CaptureWorker,
                },
                service_role: Some(*role),
                from: previous.as_str(),
                to: ServiceRuntimeState::Running.as_str(),
                origin: LifecycleOrigin::Unknown,
                detected_by,
                confidence: LifecycleConfidence::Confirmed,
                risk: 0.0,
            });
            status.last_emitted_risk = Some(transition.risk);
            status.last_transition = Some(transition.clone());
            transitions.push(transition);
        }
        LifecycleObservation::ServiceStopObserved {
            role,
            raw_reason,
            shutdown_in_progress,
            explicit_user_stop,
            detected_by,
        } => {
            let previous = status.snapshot.service_state(*role);
            let origin = if *explicit_user_stop {
                LifecycleOrigin::UserRequested
            } else if *shutdown_in_progress {
                LifecycleOrigin::SystemShutdown
            } else {
                stop_origin_from_raw_reason(raw_reason)
            };

            if previous != ServiceRuntimeState::Stopped {
                status
                    .snapshot
                    .set_service_state(*role, ServiceRuntimeState::Stopped);
                let transition = new_transition(TransitionInput {
                    domain: match role {
                        ServiceRole::PrimaryService => LifecycleDomain::PrimaryService,
                        ServiceRole::CaptureWorker => LifecycleDomain::CaptureWorker,
                    },
                    service_role: Some(*role),
                    from: previous.as_str(),
                    to: ServiceRuntimeState::Stopped.as_str(),
                    origin,
                    detected_by,
                    confidence: LifecycleConfidence::Confirmed,
                    risk: stop_risk(origin),
                });
                status.last_stop_origin = Some(origin);
                status.last_emitted_risk = Some(transition.risk);
                status.last_transition = Some(transition.clone());
                transitions.push(transition);

                if *shutdown_in_progress
                    && status.snapshot.computer_power != ComputerPowerState::ShuttingDown
                {
                    let power_transition = new_transition(TransitionInput {
                        domain: LifecycleDomain::ComputerPower,
                        service_role: None,
                        from: status.snapshot.computer_power.as_str(),
                        to: ComputerPowerState::ShuttingDown.as_str(),
                        origin: LifecycleOrigin::SystemShutdown,
                        detected_by,
                        confidence: LifecycleConfidence::Confirmed,
                        risk: 0.0,
                    });
                    status.snapshot.computer_power = ComputerPowerState::ShuttingDown;
                    status.last_emitted_risk = Some(power_transition.risk);
                    status.last_transition = Some(power_transition.clone());
                    transitions.push(power_transition);
                }
            }
        }
        LifecycleObservation::ProcessMissing {
            role,
            had_expected_runtime,
            detected_by,
        } => {
            let previous = status.snapshot.service_state(*role);
            if *had_expected_runtime && previous == ServiceRuntimeState::Running {
                status
                    .snapshot
                    .set_service_state(*role, ServiceRuntimeState::Crashed);
                let transition = new_transition(TransitionInput {
                    domain: match role {
                        ServiceRole::PrimaryService => LifecycleDomain::PrimaryService,
                        ServiceRole::CaptureWorker => LifecycleDomain::CaptureWorker,
                    },
                    service_role: Some(*role),
                    from: previous.as_str(),
                    to: ServiceRuntimeState::Crashed.as_str(),
                    origin: LifecycleOrigin::CrashOrKill,
                    detected_by,
                    confidence: LifecycleConfidence::BestEffort,
                    risk: stop_risk(LifecycleOrigin::CrashOrKill),
                });
                status.last_stop_origin = Some(LifecycleOrigin::CrashOrKill);
                status.last_emitted_risk = Some(transition.risk);
                status.last_transition = Some(transition.clone());
                transitions.push(transition);
            }
        }
        LifecycleObservation::UserSessionChanged {
            state,
            origin,
            detected_by,
        } => {
            if status.snapshot.user_session != *state {
                let transition = new_transition(TransitionInput {
                    domain: LifecycleDomain::UserSession,
                    service_role: None,
                    from: status.snapshot.user_session.as_str(),
                    to: state.as_str(),
                    origin: *origin,
                    detected_by,
                    confidence: LifecycleConfidence::Confirmed,
                    risk: 0.0,
                });
                status.snapshot.user_session = *state;
                status.last_emitted_risk = Some(transition.risk);
                status.last_transition = Some(transition.clone());
                transitions.push(transition);
            }
        }
        LifecycleObservation::CapturePermissionChanged { state, detected_by } => {
            if is_sleeping_power_state(status.snapshot.computer_power) {
                return LifecycleReduceResult {
                    status,
                    transitions,
                };
            }

            let previous = status.snapshot.capture_permission;
            if previous != *state {
                let transition = new_transition(TransitionInput {
                    domain: LifecycleDomain::CapturePermission,
                    service_role: None,
                    from: previous.as_str(),
                    to: state.as_str(),
                    origin: LifecycleOrigin::Unknown,
                    detected_by,
                    confidence: LifecycleConfidence::BestEffort,
                    risk: 0.0,
                });
                status.snapshot.capture_permission = *state;
                status.last_emitted_risk = Some(transition.risk);
                status.last_transition = Some(transition.clone());
                transitions.push(transition);
            }
        }
        LifecycleObservation::CaptureAvailabilityChanged { state, detected_by } => {
            if is_sleeping_power_state(status.snapshot.computer_power) {
                return LifecycleReduceResult {
                    status,
                    transitions,
                };
            }

            if status.snapshot.capture_availability != *state {
                let transition = new_transition(TransitionInput {
                    domain: LifecycleDomain::CaptureAvailability,
                    service_role: None,
                    from: status.snapshot.capture_availability.as_str(),
                    to: state.as_str(),
                    origin: LifecycleOrigin::Unknown,
                    detected_by,
                    confidence: LifecycleConfidence::BestEffort,
                    risk: 0.0,
                });
                status.snapshot.capture_availability = *state;
                status.last_emitted_risk = Some(transition.risk);
                status.last_transition = Some(transition.clone());
                transitions.push(transition);
            }
        }
    }

    LifecycleReduceResult {
        status,
        transitions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutdown_stop_maps_to_zero_risk() {
        let current = LifecycleStatus {
            snapshot: LifecycleSnapshot {
                computer_power: ComputerPowerState::Running,
                primary_service: ServiceRuntimeState::Running,
                ..LifecycleSnapshot::default()
            },
            ..LifecycleStatus::default()
        };

        let result = apply_observation(
            &current,
            &LifecycleObservation::ServiceStopObserved {
                role: ServiceRole::PrimaryService,
                raw_reason: "SIGTERM".to_string(),
                shutdown_in_progress: true,
                explicit_user_stop: false,
                detected_by: "signal_plus_system_state".to_string(),
            },
        );

        assert_eq!(result.transitions.len(), 2);
        assert_eq!(result.transitions[0].risk, 0.0);
        assert_eq!(
            result.transitions[0].origin,
            LifecycleOrigin::SystemShutdown
        );
        assert_eq!(
            result.transitions[0].domain,
            LifecycleDomain::PrimaryService
        );
        assert_eq!(result.transitions[1].risk, 0.0);
        assert_eq!(
            result.transitions[1].origin,
            LifecycleOrigin::SystemShutdown
        );
        assert_eq!(result.transitions[1].domain, LifecycleDomain::ComputerPower);
        assert_eq!(result.transitions[1].from, "running");
        assert_eq!(result.transitions[1].to, "shutting_down");
    }

    #[test]
    fn unknown_missing_process_maps_to_medium_risk() {
        let current = LifecycleStatus {
            snapshot: LifecycleSnapshot {
                primary_service: ServiceRuntimeState::Running,
                ..LifecycleSnapshot::default()
            },
            ..LifecycleStatus::default()
        };

        let result = apply_observation(
            &current,
            &LifecycleObservation::ProcessMissing {
                role: ServiceRole::PrimaryService,
                had_expected_runtime: true,
                detected_by: "missing_process".to_string(),
            },
        );

        assert_eq!(result.transitions.len(), 1);
        assert_eq!(result.transitions[0].risk, 0.5);
        assert_eq!(result.transitions[0].origin, LifecycleOrigin::CrashOrKill);
    }

    #[test]
    fn explicit_user_stop_transition_maps_to_zero_risk() {
        let current = LifecycleStatus {
            snapshot: LifecycleSnapshot {
                primary_service: ServiceRuntimeState::Running,
                ..LifecycleSnapshot::default()
            },
            ..LifecycleStatus::default()
        };

        let result = apply_observation(
            &current,
            &LifecycleObservation::ServiceStopObserved {
                role: ServiceRole::PrimaryService,
                raw_reason: "launchctl_bootout".to_string(),
                shutdown_in_progress: false,
                explicit_user_stop: true,
                detected_by: "stop_intent".to_string(),
            },
        );

        assert_eq!(result.transitions.len(), 1);
        assert_eq!(result.transitions[0].risk, 0.0);
        assert_eq!(result.transitions[0].origin, LifecycleOrigin::UserRequested);
    }

    #[test]
    fn suspend_transition_maps_to_zero_risk() {
        let current = LifecycleStatus {
            snapshot: LifecycleSnapshot {
                computer_power: ComputerPowerState::Running,
                ..LifecycleSnapshot::default()
            },
            ..LifecycleStatus::default()
        };

        let result = apply_observation(
            &current,
            &LifecycleObservation::ComputerPowerChanged {
                state: ComputerPowerState::Suspended,
                origin: LifecycleOrigin::SystemSuspend,
                detected_by: "login1_prepare_for_sleep".to_string(),
                confidence: LifecycleConfidence::Confirmed,
            },
        );

        assert_eq!(result.transitions.len(), 1);
        assert_eq!(result.transitions[0].risk, 0.0);
        assert_eq!(result.transitions[0].domain, LifecycleDomain::ComputerPower);
        assert_eq!(result.transitions[0].origin, LifecycleOrigin::SystemSuspend);
    }

    #[test]
    fn capture_permission_loss_transition_is_informational() {
        let current = LifecycleStatus {
            snapshot: LifecycleSnapshot {
                capture_permission: CapturePermissionState::Granted,
                ..LifecycleSnapshot::default()
            },
            ..LifecycleStatus::default()
        };

        let result = apply_observation(
            &current,
            &LifecycleObservation::CapturePermissionChanged {
                state: CapturePermissionState::Missing,
                detected_by: "test_probe".to_string(),
            },
        );

        assert_eq!(result.transitions.len(), 1);
        assert_eq!(
            result.transitions[0].domain,
            LifecycleDomain::CapturePermission
        );
        assert_eq!(result.transitions[0].from, "granted");
        assert_eq!(result.transitions[0].to, "missing");
        assert_eq!(result.transitions[0].risk, 0.0);
    }

    #[test]
    fn capture_permission_change_while_suspended_is_ignored() {
        let current = LifecycleStatus {
            snapshot: LifecycleSnapshot {
                computer_power: ComputerPowerState::Suspended,
                capture_permission: CapturePermissionState::Granted,
                ..LifecycleSnapshot::default()
            },
            ..LifecycleStatus::default()
        };

        let result = apply_observation(
            &current,
            &LifecycleObservation::CapturePermissionChanged {
                state: CapturePermissionState::Missing,
                detected_by: "failed_loop".to_string(),
            },
        );

        assert!(result.transitions.is_empty());
        assert_eq!(
            result.status.snapshot.capture_permission,
            CapturePermissionState::Granted
        );
    }

    #[test]
    fn capture_availability_change_while_suspended_is_ignored() {
        let current = LifecycleStatus {
            snapshot: LifecycleSnapshot {
                computer_power: ComputerPowerState::Suspended,
                capture_availability: CaptureAvailabilityState::Ready,
                ..LifecycleSnapshot::default()
            },
            ..LifecycleStatus::default()
        };

        let result = apply_observation(
            &current,
            &LifecycleObservation::CaptureAvailabilityChanged {
                state: CaptureAvailabilityState::Blocked,
                detected_by: "failed_loop".to_string(),
            },
        );

        assert!(result.transitions.is_empty());
        assert_eq!(
            result.status.snapshot.capture_availability,
            CaptureAvailabilityState::Ready
        );
    }

    #[test]
    fn capture_permission_gain_transition_is_informational() {
        let current = LifecycleStatus {
            snapshot: LifecycleSnapshot {
                capture_permission: CapturePermissionState::Missing,
                ..LifecycleSnapshot::default()
            },
            ..LifecycleStatus::default()
        };

        let result = apply_observation(
            &current,
            &LifecycleObservation::CapturePermissionChanged {
                state: CapturePermissionState::Granted,
                detected_by: "test_probe".to_string(),
            },
        );

        assert_eq!(result.transitions.len(), 1);
        assert_eq!(result.transitions[0].risk, 0.0);
    }
}
