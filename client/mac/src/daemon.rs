use std::ffi::c_void;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use block2::RcBlock;
use chrono::{DateTime, Utc};
use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSWorkspace, NSWorkspaceWillPowerOffNotification};
use objc2_foundation::{NSDate, NSRunLoop};
use tokio::sync::mpsc;
use tokio::time::sleep;
use virtue_core::storage::FileStateStore;
use virtue_core::{
    CaptureAvailabilityState, CapturePermissionState, ComputerPowerState, LifecycleConfidence,
    LifecycleObservation, LifecycleOrigin, MonitorService, PlatformHooks, ServiceRole,
    ServiceStopMarker,
};

use crate::capture::{MacPlatformHooks, has_screen_capture_access, is_permission_missing_error};
use crate::config::{ClientPaths, build_core_config};

const IDLE_LOOP_INTERVAL: Duration = Duration::from_secs(1);
const ERROR_RETRY_INTERVAL: Duration = Duration::from_secs(20);
const SERVICE_PING_WAKE_PADDING_MS: i64 = 1_000;
const POST_WAKE_CAPTURE_STATE_SUPPRESSION: Duration = Duration::from_secs(30);
const SHUTDOWN_REBOOT_RECOVERY_WINDOW_MS: i64 = 120_000;
const K_IO_MESSAGE_CAN_SYSTEM_SLEEP: u32 = 3_758_097_008;
const K_IO_MESSAGE_SYSTEM_WILL_SLEEP: u32 = 3_758_097_024;
const K_IO_MESSAGE_SYSTEM_WILL_NOT_SLEEP: u32 = 3_758_097_040;
const K_IO_MESSAGE_SYSTEM_HAS_POWERED_ON: u32 = 3_758_097_152;
const K_IO_MESSAGE_SYSTEM_WILL_POWER_ON: u32 = 3_758_097_184;

type IoObject = u32;
type IoConnect = u32;
type IoService = u32;
type IoNotificationPortRef = *mut c_void;
type CfRunLoopRef = *const c_void;
type CfRunLoopSourceRef = *const c_void;
type CfStringRef = *const c_void;

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IORegisterForSystemPower(
        refcon: *mut c_void,
        the_port_ref: *mut IoNotificationPortRef,
        callback: unsafe extern "C" fn(*mut c_void, IoService, u32, *mut c_void),
        notifier: *mut IoObject,
    ) -> IoConnect;
    fn IODeregisterForSystemPower(notifier: *mut IoObject) -> i32;
    fn IOAllowPowerChange(kernel_port: IoConnect, notification_id: isize) -> i32;
    fn IONotificationPortDestroy(notify: IoNotificationPortRef);
    fn IONotificationPortGetRunLoopSource(notify: IoNotificationPortRef) -> CfRunLoopSourceRef;
    fn IOServiceClose(connect: IoConnect) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFRunLoopCommonModes: CfStringRef;
    fn CFRunLoopGetCurrent() -> CfRunLoopRef;
    fn CFRunLoopAddSource(rl: CfRunLoopRef, source: CfRunLoopSourceRef, mode: CfStringRef);
    fn CFRunLoopRun();
}

#[derive(Clone, Copy, Debug)]
enum PowerEvent {
    WillPowerOff,
    WillSleep,
    DidWake,
}

struct PowerNotificationContext {
    event_tx: mpsc::UnboundedSender<PowerEvent>,
    root_port: IoConnect,
    notify_port: IoNotificationPortRef,
    notifier: IoObject,
}

struct ShutdownWatcher {
    should_stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl ShutdownWatcher {
    fn spawn(event_tx: mpsc::UnboundedSender<PowerEvent>) -> Self {
        let should_stop = Arc::new(AtomicBool::new(false));
        let worker_stop = should_stop.clone();
        let worker = thread::spawn(move || {
            autoreleasepool(|_| unsafe {
                let workspace = NSWorkspace::sharedWorkspace();
                let center = workspace.notificationCenter();
                let callback = RcBlock::new(move |_| {
                    let _ = event_tx.send(PowerEvent::WillPowerOff);
                });
                let observer = center.addObserverForName_object_queue_usingBlock(
                    Some(NSWorkspaceWillPowerOffNotification),
                    None,
                    None,
                    &callback,
                );
                let run_loop = NSRunLoop::currentRunLoop();

                while !worker_stop.load(Ordering::SeqCst) {
                    let next_tick = NSDate::dateWithTimeIntervalSinceNow(0.5);
                    run_loop.runUntilDate(&next_tick);
                }

                center.removeObserver((*observer).as_ref());
            });
        });

        Self {
            should_stop,
            worker: Some(worker),
        }
    }
}

impl Drop for ShutdownWatcher {
    fn drop(&mut self) {
        self.should_stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub fn run_daemon(paths: &ClientPaths) -> Result<()> {
    let paths = paths.clone();
    let (power_tx, power_rx) = mpsc::unbounded_channel::<PowerEvent>();

    spawn_power_notification_watcher(power_tx.clone());
    let _shutdown_watcher = ShutdownWatcher::spawn(power_tx);

    let result = tokio::runtime::Runtime::new()
        .map_err(anyhow::Error::from)
        .and_then(|runtime| runtime.block_on(run_daemon_service_loop(&paths, power_rx)));
    if let Err(err) = result {
        eprintln!("daemon: {err:#}");
        std::process::exit(1);
    }
    std::process::exit(0);
}

fn spawn_power_notification_watcher(event_tx: mpsc::UnboundedSender<PowerEvent>) {
    thread::spawn(move || {
        let mut context = Box::new(PowerNotificationContext {
            event_tx,
            root_port: 0,
            notify_port: std::ptr::null_mut(),
            notifier: 0,
        });

        let refcon = (&mut *context) as *mut PowerNotificationContext as *mut c_void;

        context.root_port = unsafe {
            IORegisterForSystemPower(
                refcon,
                &mut context.notify_port,
                power_notification_callback,
                &mut context.notifier,
            )
        };
        if context.root_port == 0 || context.notify_port.is_null() {
            return;
        }

        let source = unsafe { IONotificationPortGetRunLoopSource(context.notify_port) };
        if source.is_null() {
            unsafe { cleanup_power_notification_context(&mut context) };
            return;
        }

        unsafe { CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopCommonModes) };
        unsafe { CFRunLoopRun() };

        unsafe { cleanup_power_notification_context(&mut context) };
    });
}

unsafe extern "C" fn power_notification_callback(
    refcon: *mut c_void,
    _service: IoService,
    message_type: u32,
    message_argument: *mut c_void,
) {
    let Some(context) = (unsafe { (refcon as *mut PowerNotificationContext).as_mut() }) else {
        return;
    };

    match message_type {
        K_IO_MESSAGE_CAN_SYSTEM_SLEEP => {
            let _ = unsafe { IOAllowPowerChange(context.root_port, message_argument as isize) };
        }
        K_IO_MESSAGE_SYSTEM_WILL_SLEEP => {
            let _ = context.event_tx.send(PowerEvent::WillSleep);
            let _ = unsafe { IOAllowPowerChange(context.root_port, message_argument as isize) };
        }
        K_IO_MESSAGE_SYSTEM_WILL_NOT_SLEEP => {}
        K_IO_MESSAGE_SYSTEM_WILL_POWER_ON => {}
        K_IO_MESSAGE_SYSTEM_HAS_POWERED_ON => {
            let _ = context.event_tx.send(PowerEvent::DidWake);
        }
        _ => {}
    }
}

unsafe fn cleanup_power_notification_context(context: &mut PowerNotificationContext) {
    if context.notifier != 0 {
        let _ = unsafe { IODeregisterForSystemPower(&mut context.notifier) };
        context.notifier = 0;
    }
    if !context.notify_port.is_null() {
        unsafe { IONotificationPortDestroy(context.notify_port) };
        context.notify_port = std::ptr::null_mut();
    }
    if context.root_port != 0 {
        let _ = unsafe { IOServiceClose(context.root_port) };
        context.root_port = 0;
    }
}

async fn run_daemon_service_loop(
    paths: &ClientPaths,
    mut power_rx: mpsc::UnboundedReceiver<PowerEvent>,
) -> Result<()> {
    paths.ensure_dirs()?;

    let shutdown = Arc::new(AtomicBool::new(false));
    let system_shutdown_requested = Arc::new(AtomicBool::new(false));
    let (signal_tx, mut signal_rx) = mpsc::unbounded_channel::<String>();
    spawn_signal_handler(shutdown.clone(), signal_tx);

    let boot_info = current_boot_marker();
    let recovered_shutdown = if let Some((boot_marker, started_at)) = boot_info.as_ref() {
        recover_recent_shutdown_stop_marker(paths, boot_marker, started_at.timestamp_millis())?
    } else {
        false
    };

    let mut service = MonitorService::setup(build_core_config(paths), MacPlatformHooks::new())?;
    if recovered_shutdown {
        recover_shutdown_transition(&mut service);
    }
    if let Some((boot_marker, started_at)) = boot_info {
        let _ = service.record_lifecycle_observation(LifecycleObservation::BootObserved {
            boot_marker,
            booted_at_ms: Some(started_at.timestamp_millis()),
            detected_by: "kern_boottime_change".to_string(),
        });
    }
    let _ = service.record_lifecycle_observation(LifecycleObservation::ServiceStarted {
        role: ServiceRole::PrimaryService,
        detected_by: "macos_launch_agent".to_string(),
    });
    record_capture_state(&mut service, "startup_probe", CaptureState::current());

    let mut sleeping = false;
    let mut suppress_capture_state_until: Option<Instant> = None;
    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        let sleep_duration = if sleeping {
            IDLE_LOOP_INTERVAL
        } else {
            let sleep_duration = match service.loop_iteration() {
                Ok(outcome) => {
                    record_capture_state(&mut service, "successful_loop", CaptureState::current());
                    duration_until(outcome.next_run_at_ms)
                }
                Err(err) => {
                    let error_text = err.to_string();
                    if suppress_capture_state_until.is_none_or(|until| Instant::now() >= until) {
                        if is_permission_missing_error(&error_text) {
                            record_capture_state(
                                &mut service,
                                "failed_loop",
                                CaptureState::permission_missing(),
                            );
                        } else {
                            record_capture_state(
                                &mut service,
                                "failed_loop",
                                CaptureState::blocked_with_current_permission(),
                            );
                        }
                    }
                    eprintln!("daemon: {error_text}");
                    ERROR_RETRY_INTERVAL
                }
            };
            let _ = service
                .record_service_ping_if_due(ServiceRole::PrimaryService, "macos_service_timer");
            service
                .next_service_ping_due_at_ms(ServiceRole::PrimaryService)
                .ok()
                .flatten()
                .map(|due_at_ms| {
                    duration_until(due_at_ms.saturating_add(SERVICE_PING_WAKE_PADDING_MS))
                })
                .map(|ping_duration| ping_duration.min(sleep_duration))
                .unwrap_or(sleep_duration)
        };

        tokio::select! {
            signal = signal_rx.recv() => {
                if let Some(signal_name) = signal {
                    let explicit_user_stop = service
                        .take_stop_intent(ServiceRole::PrimaryService)
                        .ok()
                        .flatten()
                        .is_some();
                    let _ = service.record_lifecycle_observation(
                        LifecycleObservation::ServiceStopObserved {
                            role: ServiceRole::PrimaryService,
                            raw_reason: signal_name,
                            shutdown_in_progress: system_shutdown_requested.load(Ordering::SeqCst),
                            explicit_user_stop,
                            detected_by: "signal_plus_power_notification".to_string(),
                        },
                    );
                }
                break;
            }
            power_event = power_rx.recv() => {
                match power_event {
                    Some(PowerEvent::WillPowerOff) => {
                        system_shutdown_requested.store(true, Ordering::SeqCst);
                        record_power_change(
                            &mut service,
                            ComputerPowerState::ShuttingDown,
                            LifecycleOrigin::SystemShutdown,
                            "nsworkspace_will_power_off",
                        );
                        let _ = service.record_lifecycle_observation(
                            LifecycleObservation::ServiceStopObserved {
                                role: ServiceRole::PrimaryService,
                                raw_reason: "NSWorkspaceWillPowerOffNotification".to_string(),
                                shutdown_in_progress: true,
                                explicit_user_stop: false,
                                detected_by: "nsworkspace_will_power_off".to_string(),
                            },
                        );
                        shutdown.store(true, Ordering::SeqCst);
                    }
                    Some(PowerEvent::WillSleep) => {
                        sleeping = true;
                        suppress_capture_state_until = None;
                        record_power_change(
                            &mut service,
                            ComputerPowerState::Suspended,
                            LifecycleOrigin::SystemSuspend,
                            "iokit_system_will_sleep",
                        );
                    }
                    Some(PowerEvent::DidWake) => {
                        sleeping = false;
                        suppress_capture_state_until =
                            Some(Instant::now() + POST_WAKE_CAPTURE_STATE_SUPPRESSION);
                        record_power_change(
                            &mut service,
                            ComputerPowerState::Running,
                            LifecycleOrigin::SystemSuspend,
                            "iokit_system_has_powered_on",
                        );
                        let _ = service.upload_pending_batch_now();
                    }
                    None => {}
                }
            }
            _ = sleep_interruptible(&shutdown, sleep_duration) => {}
        }
    }

    let _ = service.shutdown();
    Ok(())
}

fn record_power_change<P: PlatformHooks>(
    service: &mut MonitorService<P>,
    state: ComputerPowerState,
    origin: LifecycleOrigin,
    detected_by: &str,
) {
    let _ = service.record_lifecycle_observation(LifecycleObservation::ComputerPowerChanged {
        state,
        origin,
        detected_by: detected_by.to_string(),
        confidence: LifecycleConfidence::Confirmed,
    });
}

fn recover_shutdown_transition<P: PlatformHooks>(service: &mut MonitorService<P>) {
    let _ = service.record_lifecycle_observation(LifecycleObservation::ComputerPowerChanged {
        state: ComputerPowerState::ShuttingDown,
        origin: LifecycleOrigin::SystemShutdown,
        detected_by: "boot_marker_plus_recent_stop_marker".to_string(),
        confidence: LifecycleConfidence::BestEffort,
    });
}

#[derive(Clone, Copy, Debug)]
struct CaptureState {
    permission: CapturePermissionState,
    availability: CaptureAvailabilityState,
}

impl CaptureState {
    fn current() -> Self {
        let permission = current_permission_state();
        let availability = match permission {
            CapturePermissionState::Granted => CaptureAvailabilityState::Ready,
            CapturePermissionState::Missing => CaptureAvailabilityState::Blocked,
            CapturePermissionState::Unsupported | CapturePermissionState::Unknown => {
                CaptureAvailabilityState::Unknown
            }
        };
        Self {
            permission,
            availability,
        }
    }

    fn permission_missing() -> Self {
        Self {
            permission: CapturePermissionState::Missing,
            availability: CaptureAvailabilityState::Blocked,
        }
    }

    fn blocked_with_current_permission() -> Self {
        Self {
            permission: current_permission_state(),
            availability: CaptureAvailabilityState::Blocked,
        }
    }
}

fn current_permission_state() -> CapturePermissionState {
    if has_screen_capture_access() {
        CapturePermissionState::Granted
    } else {
        CapturePermissionState::Missing
    }
}

fn record_capture_state<P: PlatformHooks>(
    service: &mut MonitorService<P>,
    detected_by: &str,
    state: CaptureState,
) {
    let _ = service.record_lifecycle_observation(LifecycleObservation::CapturePermissionChanged {
        state: state.permission,
        detected_by: detected_by.to_string(),
    });
    let _ =
        service.record_lifecycle_observation(LifecycleObservation::CaptureAvailabilityChanged {
            state: state.availability,
            detected_by: detected_by.to_string(),
        });
}

fn spawn_signal_handler(shutdown: Arc<AtomicBool>, signal_tx: mpsc::UnboundedSender<String>) {
    tokio::spawn(async move {
        use tokio::signal::unix::{SignalKind, signal};

        let mut sigterm = match signal(SignalKind::terminate()) {
            Ok(signal) => signal,
            Err(_) => return,
        };
        let mut sigint = signal(SignalKind::interrupt()).ok();

        let signal_name = tokio::select! {
            _ = sigterm.recv() => "SIGTERM",
            _ = async {
                match sigint.as_mut() {
                    Some(signal) => signal.recv().await,
                    None => std::future::pending::<Option<()>>().await,
                }
            } => "SIGINT",
        };

        shutdown.store(true, Ordering::SeqCst);
        let _ = signal_tx.send(signal_name.to_string());
    });
}

async fn sleep_interruptible(shutdown: &Arc<AtomicBool>, duration: Duration) {
    let mut remaining = duration;
    while remaining > Duration::ZERO && !shutdown.load(Ordering::SeqCst) {
        let tick = remaining.min(Duration::from_secs(1));
        sleep(tick).await;
        remaining = remaining.saturating_sub(tick);
    }
}

fn duration_until(next_run_at_ms: i64) -> Duration {
    let now_ms = Utc::now().timestamp_millis();
    let delta_ms = next_run_at_ms.saturating_sub(now_ms);
    Duration::from_millis(delta_ms.max(0) as u64)
}

fn recover_recent_shutdown_stop_marker(
    paths: &ClientPaths,
    current_boot_marker: &str,
    booted_at_ms: i64,
) -> Result<bool> {
    let store = FileStateStore::new(&paths.state_dir)?;
    let previous_boot_marker = store
        .load_status()?
        .and_then(|status| status.lifecycle.last_boot_marker);
    let marker = store.load_service_stop_marker(ServiceRole::PrimaryService)?;

    if !should_recover_shutdown_stop_marker(
        previous_boot_marker.as_deref(),
        current_boot_marker,
        booted_at_ms,
        marker.as_ref(),
    ) {
        return Ok(false);
    }

    let Some(marker) = marker else {
        return Ok(false);
    };

    let mut recovered_marker = marker;
    recovered_marker.origin = LifecycleOrigin::SystemShutdown;
    store.save_service_stop_marker(&recovered_marker)?;
    if let Some(mut status) = store.load_status()? {
        status.lifecycle.last_stop_origin = Some(LifecycleOrigin::SystemShutdown);
        if let Some(transition) = status.lifecycle.last_transition.as_mut()
            && transition.service_role == Some(ServiceRole::PrimaryService)
            && transition.to == "stopped"
        {
            transition.origin = LifecycleOrigin::SystemShutdown;
            transition.risk = 0.0;
        }
        if matches!(status.lifecycle.last_emitted_risk, Some(risk) if risk > 0.0) {
            status.lifecycle.last_emitted_risk = Some(0.0);
        }
        store.save_status(&status)?;
    }
    Ok(true)
}

fn should_recover_shutdown_stop_marker(
    previous_boot_marker: Option<&str>,
    current_boot_marker: &str,
    booted_at_ms: i64,
    marker: Option<&ServiceStopMarker>,
) -> bool {
    if previous_boot_marker.is_none() || previous_boot_marker == Some(current_boot_marker) {
        return false;
    }

    let Some(marker) = marker else {
        return false;
    };

    if marker.origin != LifecycleOrigin::Unknown || marker.stopped_at_ms > booted_at_ms {
        return false;
    }

    booted_at_ms.saturating_sub(marker.stopped_at_ms) <= SHUTDOWN_REBOOT_RECOVERY_WINDOW_MS
}

fn current_boot_marker() -> Option<(String, DateTime<Utc>)> {
    let output = Command::new("/usr/sbin/sysctl")
        .args(["-n", "kern.boottime"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let seconds = parse_sysctl_component(&stdout, "sec = ")?;
    let usec = parse_sysctl_component(&stdout, "usec = ")?;
    let started_at = DateTime::<Utc>::from_timestamp(seconds, (usec as u32).saturating_mul(1_000))?;
    let boot_id = format!("{seconds}:{usec}");
    Some((boot_id, started_at))
}

fn parse_sysctl_component(text: &str, prefix: &str) -> Option<i64> {
    let start = text.find(prefix)? + prefix.len();
    let rest = &text[start..];
    let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stop_marker(origin: LifecycleOrigin, stopped_at_ms: i64) -> ServiceStopMarker {
        ServiceStopMarker {
            role: ServiceRole::PrimaryService,
            origin,
            stopped_at_ms,
        }
    }

    #[test]
    fn recovers_unknown_stop_when_reboot_follows_quickly() {
        assert!(should_recover_shutdown_stop_marker(
            Some("boot-a"),
            "boot-b",
            30_000,
            Some(&stop_marker(LifecycleOrigin::Unknown, 5_000)),
        ));
    }

    #[test]
    fn does_not_recover_without_new_boot_marker() {
        assert!(!should_recover_shutdown_stop_marker(
            Some("boot-a"),
            "boot-a",
            30_000,
            Some(&stop_marker(LifecycleOrigin::Unknown, 5_000)),
        ));
    }

    #[test]
    fn does_not_recover_long_preboot_gap() {
        assert!(!should_recover_shutdown_stop_marker(
            Some("boot-a"),
            "boot-b",
            SHUTDOWN_REBOOT_RECOVERY_WINDOW_MS + 5_001,
            Some(&stop_marker(LifecycleOrigin::Unknown, 5_000)),
        ));
    }

    #[test]
    fn does_not_recover_non_unknown_stop_origins() {
        assert!(!should_recover_shutdown_stop_marker(
            Some("boot-a"),
            "boot-b",
            30_000,
            Some(&stop_marker(LifecycleOrigin::UserRequested, 5_000)),
        ));
    }
}
