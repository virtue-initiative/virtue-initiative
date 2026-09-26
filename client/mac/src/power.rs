//! System wake notifications via `IORegisterForSystemPower`.
//!
//! Push-based on purpose: the daemon should do no work at all while the
//! machine is awake and idle, and nothing here ever schedules a timer, so it
//! can't be what wakes the machine from sleep. The watcher thread sits in
//! `CFRunLoopRun` until IOKit delivers a power message.

use std::ffi::c_void;
use std::thread;

use tokio::sync::mpsc;

type IoObject = u32;
type IoConnect = u32;
type IoService = u32;
type IoNotificationPortRef = *mut c_void;
type CfRunLoopRef = *const c_void;
type CfRunLoopSourceRef = *const c_void;
type CfStringRef = *const c_void;

// `kIOMessage*` values from <IOKit/IOMessage.h>.
const K_IO_MESSAGE_CAN_SYSTEM_SLEEP: u32 = 0xE000_0270;
const K_IO_MESSAGE_SYSTEM_WILL_SLEEP: u32 = 0xE000_0280;
const K_IO_MESSAGE_SYSTEM_HAS_POWERED_ON: u32 = 0xE000_0300;

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IORegisterForSystemPower(
        refcon: *mut c_void,
        the_port_ref: *mut IoNotificationPortRef,
        callback: unsafe extern "C" fn(*mut c_void, IoService, u32, *mut c_void),
        notifier: *mut IoObject,
    ) -> IoConnect;
    fn IOAllowPowerChange(kernel_port: IoConnect, notification_id: isize) -> i32;
    fn IONotificationPortGetRunLoopSource(notify: IoNotificationPortRef) -> CfRunLoopSourceRef;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFRunLoopCommonModes: CfStringRef;
    fn CFRunLoopGetCurrent() -> CfRunLoopRef;
    fn CFRunLoopAddSource(rl: CfRunLoopRef, source: CfRunLoopSourceRef, mode: CfStringRef);
    fn CFRunLoopRun();
}

struct WakeWatcher {
    wake_tx: mpsc::UnboundedSender<()>,
    root_port: IoConnect,
    notify_port: IoNotificationPortRef,
    notifier: IoObject,
}

/// Sends `()` on `wake_tx` each time the system finishes waking from sleep.
/// Runs for the life of the process. If registration fails the watcher
/// thread exits and drops `wake_tx`, so the receiver sees the channel close
/// rather than waiting on it forever.
pub fn spawn_wake_watcher(wake_tx: mpsc::UnboundedSender<()>) {
    thread::spawn(move || {
        // Leaked: IOKit holds `refcon` for as long as the registration lives,
        // which is the rest of the process.
        let watcher = Box::leak(Box::new(WakeWatcher {
            wake_tx,
            root_port: 0,
            notify_port: std::ptr::null_mut(),
            notifier: 0,
        }));
        let refcon = watcher as *mut WakeWatcher as *mut c_void;

        // SAFETY: `refcon` and both out-params point into the leaked
        // `WakeWatcher`, which outlives the registration.
        watcher.root_port = unsafe {
            IORegisterForSystemPower(
                refcon,
                &mut watcher.notify_port,
                power_callback,
                &mut watcher.notifier,
            )
        };
        if watcher.root_port == 0 || watcher.notify_port.is_null() {
            tracing::warn!("IORegisterForSystemPower failed; no post-wake batch flush");
            return;
        }

        // SAFETY: `notify_port` was just returned non-null by IOKit.
        let source = unsafe { IONotificationPortGetRunLoopSource(watcher.notify_port) };
        if source.is_null() {
            tracing::warn!("power notification port has no run loop source");
            return;
        }

        // SAFETY: plain CoreFoundation calls on this thread's own run loop.
        unsafe {
            CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopCommonModes);
            CFRunLoopRun();
        }
    });
}

unsafe extern "C" fn power_callback(
    refcon: *mut c_void,
    _service: IoService,
    message_type: u32,
    message_argument: *mut c_void,
) {
    // SAFETY: `refcon` is the leaked `WakeWatcher` passed at registration.
    let Some(watcher) = (unsafe { (refcon as *const WakeWatcher).as_ref() }) else {
        return;
    };

    match message_type {
        // Registering for system power obliges us to acknowledge these, or
        // sleep stalls for up to 30s waiting on us.
        K_IO_MESSAGE_CAN_SYSTEM_SLEEP | K_IO_MESSAGE_SYSTEM_WILL_SLEEP => {
            // SAFETY: `root_port` is the live connection from registration,
            // and `message_argument` is the notification ID IOKit expects back.
            let _ = unsafe { IOAllowPowerChange(watcher.root_port, message_argument as isize) };
        }
        K_IO_MESSAGE_SYSTEM_HAS_POWERED_ON => {
            let _ = watcher.wake_tx.send(());
        }
        _ => {}
    }
}
