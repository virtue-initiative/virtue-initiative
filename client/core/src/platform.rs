use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{CoreError, CoreResult};
use crate::model::Screenshot;

/// Object-safe subset of platform capabilities. Used as a trait object
/// (`Box<dyn ScreenshotHooks>`) in modules.
pub trait ScreenshotHooks: Send + Sync + 'static {
    fn take_screenshot(&self) -> CoreResult<Screenshot>;

    fn get_time_utc_ms(&self) -> CoreResult<i64> {
        let duration = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| CoreError::CommandFailed(e.to_string()))?;
        i64::try_from(duration.as_millis())
            .map_err(|_| CoreError::InvalidState("system clock overflow"))
    }

    /// True if the session is locked or a screensaver is active.
    ///
    /// While locked/screensaving the user cannot be viewing real content, so the
    /// screenshot module suppresses capture unconditionally. `Ok(false)` is the
    /// fail-safe default when the state can't be determined (unknown session) —
    /// i.e. fall back to the diff gate, never silently suppress. The default
    /// implementation returns `Ok(false)` for platforms without the concept
    /// (e.g. mobile); desktop platforms override it.
    fn is_locked_or_screensaver(&self) -> CoreResult<bool> {
        Ok(false)
    }

    /// Whether *this process* can actually service a queued
    /// `ScreenshotState::force_capture_requested` request right now. `true`
    /// for every normal platform (the default) — the process that owns the
    /// daemon loop is also the one with a real `take_screenshot()`. iOS is
    /// the sole exception: the main app process and the Safari extension
    /// process each run their own independent `Daemon` against the same
    /// on-disk state (CORE-016), but only the extension process has a real
    /// frame source — the app target's `take_screenshot()` is a hardcoded
    /// stub that always fails (see `client/ios/app/Sources/CaptureCallbacks.swift`).
    /// Without this, the app process's own tick (e.g. the one `run_forever`
    /// runs right after servicing a login/logout/pause/resume request) would
    /// immediately consume and fail a request meant for the extension
    /// process to service on its next real heartbeat.
    fn can_force_capture_now(&self) -> bool {
        true
    }
}

/// Live, per-tick I/O feeding the lifecycle late-wakeup model. See
/// `daemon::lifecycle` for how these hooks are used.
pub trait LifecycleHooks: Send + Sync + 'static {
    /// Same semantics/default as `ScreenshotHooks::get_time_utc_ms`.
    fn get_utc_clock_ms(&self) -> CoreResult<i64> {
        let duration = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| CoreError::CommandFailed(e.to_string()))?;
        i64::try_from(duration.as_millis())
            .map_err(|_| CoreError::InvalidState("system clock overflow"))
    }

    /// A clock that does not advance while the system is suspended — used
    /// only by `lifecycle::tick`'s suspend evidence (CORE-002), never in
    /// place of `get_utc_clock_ms` itself. Default falls back to
    /// `get_utc_clock_ms`: on a platform with no distinct suspend-safe
    /// primitive, suspend evidence simply never triggers, which is safe (it
    /// can only ever *add* an excuse, never remove one).
    fn get_monotonic_clock_ms(&self) -> CoreResult<i64> {
        self.get_utc_clock_ms()
    }

    /// Start of the current expected-running window (OS session/user login),
    /// as a UTC timestamp. `None` if not yet knowable.
    fn get_last_login_utc_ms(&self) -> CoreResult<Option<i64>>;

    /// End of the most recently closed expected-running window (OS
    /// session/user logout), as a UTC timestamp. `None` while the current
    /// session is presumed still open.
    fn get_last_logout_utc_ms(&self) -> CoreResult<Option<i64>>;

    /// Whether the late-wakeup tamper model (CORE-002)
    /// applies on this platform at all — skips `lifecycle::tick` entirely
    /// when `false`. Only iOS returns `false`: its Safari extension host can
    /// be suspended the instant the device locks with no notification and no
    /// boot/shutdown/session API at all, so every stall looks identical and
    /// there's no meaningful "late wakeup" signal to build there.
    fn lifecycle_enabled(&self) -> bool {
        true
    }
}

/// Marker trait for platform implementations. Kept as a supertrait rather
/// than requiring call sites to name both `ScreenshotHooks` and
/// `LifecycleHooks` individually.
pub trait PlatformHooks: ScreenshotHooks + LifecycleHooks {}

impl<T: ScreenshotHooks + LifecycleHooks> PlatformHooks for T {}
