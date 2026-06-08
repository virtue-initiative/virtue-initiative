use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use block2::RcBlock;
use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSWorkspace, NSWorkspaceWillPowerOffNotification};
use tokio::sync::mpsc;
use virtue_core::events::{Event, ProcessStoppedReason};
use virtue_core::ipc::is_allowed_inbound;
use virtue_core::{MonitorService, iter_sleep};

use crate::capture::{MacEvent, MacPlatformHooks, has_screen_capture_access, is_permission_missing_error};
use crate::capture_reporter::CaptureReporterObserver;
use crate::config::{ClientPaths, build_core_config};

const POST_WAKE_CAPTURE_STATE_SUPPRESSION: Duration = Duration::from_secs(30);

type IoObject = u32;
type IoConnect = u32;
type IoService = u32;
type IoNotificationPortRef = *mut c_void;
type CfRunLoopRef = *const c_void;
type CfRunLoopSourceRef = *const c_void;
type CfStringRef = *const c_void;

const K_IO_MESSAGE_CAN_SYSTEM_SLEEP: u32 = 3_758_097_008;
const K_IO_MESSAGE_SYSTEM_WILL_SLEEP: u32 = 3_758_097_024;
const K_IO_MESSAGE_SYSTEM_WILL_NOT_SLEEP: u32 = 3_758_097_040;
const K_IO_MESSAGE_SYSTEM_HAS_POWERED_ON: u32 = 3_758_097_152;
const K_IO_MESSAGE_SYSTEM_WILL_POWER_ON: u32 = 3_758_097_184;

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
                while !worker_stop.load(Ordering::SeqCst) {
                    thread::sleep(Duration::from_millis(500));
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

    let mut service = MonitorService::setup(build_core_config(paths), MacPlatformHooks::new())?;

    // Register the Mac-specific capture availability reporter.
    let capture_reporter =
        CaptureReporterObserver::new(service.event_queue_sender());
    service.add_observer(Box::new(capture_reporter));

    service.queue_event(Event::ProcessStarted);
    let _ = service.run_event_loop_iter();

    // Bind IPC listener and spawn an accept thread.
    let sock_path = paths.state_dir.join("daemon.sock");
    let (ipc_accept_tx, mut ipc_accept_rx) =
        mpsc::unbounded_channel::<(virtue_core::ipc::IpcSender, virtue_core::ipc::IpcReceiver)>();
    if let Ok(listener) = service.bind_ipc(&sock_path) {
        tokio::task::spawn_blocking(move || {
            loop {
                match listener.blocking_accept() {
                    Ok(pair) => {
                        if ipc_accept_tx.send(pair).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        eprintln!("daemon: ipc accept error: {e}");
                        break;
                    }
                }
            }
        });
    } else {
        eprintln!(
            "daemon: failed to bind IPC listener at {}",
            sock_path.display()
        );
    }

    let mut sleeping = false;
    let mut suppress_capture_state_until: Option<Instant> = None;
    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        // Wire up any newly accepted IPC connections.
        while let Ok((sender, mut receiver)) = ipc_accept_rx.try_recv() {
            service.add_ipc_client(sender);
            let event_tx = service.event_queue_sender();
            std::thread::spawn(move || {
                while let Ok(event) = receiver.recv_event::<MacEvent>() {
                    if is_allowed_inbound(&event) {
                        event_tx.send(event).ok();
                    }
                }
            });
        }

        if !sleeping {
            match tokio::task::block_in_place(|| service.loop_iteration()) {
                Ok(_outcome) => {
                    if !has_screen_capture_access() {
                        suppress_capture_state_until = None;
                    }
                }
                Err(err) => {
                    let error_text = err.to_string();
                    if suppress_capture_state_until.is_none_or(|until| Instant::now() >= until) {
                        if is_permission_missing_error(&error_text) {
                            eprintln!("daemon: capture permission missing: {error_text}");
                        } else {
                            eprintln!("daemon: capture blocked: {error_text}");
                        }
                    }
                    eprintln!("daemon: {error_text}");
                }
            }
        }

        tokio::select! {
            signal = signal_rx.recv() => {
                if let Some(_signal_name) = signal {
                    let explicit_user_stop = service.consume_user_stop_request();
                    let reason = if system_shutdown_requested.load(Ordering::SeqCst) {
                        ProcessStoppedReason::Shutdown
                    } else if explicit_user_stop {
                        ProcessStoppedReason::User
                    } else {
                        ProcessStoppedReason::Other
                    };
                    service.queue_event(Event::ProcessStopped(reason));
                    let _ = service.run_event_loop_iter();
                }
                break;
            }
            power_event = power_rx.recv() => {
                match power_event {
                    Some(PowerEvent::WillPowerOff) => {
                        system_shutdown_requested.store(true, Ordering::SeqCst);
                        service.queue_event(Event::ProcessStopped(ProcessStoppedReason::Shutdown));
                        let _ = service.run_event_loop_iter();
                        shutdown.store(true, Ordering::SeqCst);
                    }
                    Some(PowerEvent::WillSleep) => {
                        sleeping = true;
                        suppress_capture_state_until = None;
                        service.queue_event(Event::ComputerSuspended);
                        let _ = service.run_event_loop_iter();
                    }
                    Some(PowerEvent::DidWake) => {
                        sleeping = false;
                        suppress_capture_state_until =
                            Some(Instant::now() + POST_WAKE_CAPTURE_STATE_SUPPRESSION);
                        service.queue_event(Event::ComputerResumed);
                        let _ = service.run_event_loop_iter();
                        let _ = service.upload_pending_batch_now();
                    }
                    None => {}
                }
            }
            _ = iter_sleep() => {}
        }
    }

    let _ = service.run_event_loop_iter();
    let _ = service.mark_stopped();
    Ok(())
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
