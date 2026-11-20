# Tamper detection

How the core notices that monitoring was interrupted, and what it can't notice. The exact rules
are in `SPEC.md`; this page explains how they fit together. Implementation:
`client/core/src/module/lifecycle.rs`, `client/core/src/module/capture_availability.rs` and
`client/core/src/clock.rs`.

## Late wakeups (CORE-002)

The core schedules every wakeup, so it knows when it should next be running. Each tick compares
the actual time to the time it was scheduled for and appends the difference to an array of the
last 10 wakeups (`LifecycleState.late_wakeups`). A gap is excused, and left out of the array,
when:

- the wakeup is within 2 minutes of a system login, or was scheduled within 2 minutes of a
  system logout, and neither of those two signals contradicts the other;
- a suspend accounts for essentially the whole gap. The core detects a suspend by comparing the
  boot clock, which counts time suspended, with a clock that doesn't. Suspend evidence can
  excuse a gap by itself, and it never blocks a login or logout excuse;
- monitoring was stopped from the app (see below).

An alert (`UploadKind::ScreenshotMissed`, `HIGH_RISK_LIFECYCLE_ALERT`) fires when a single entry
exceeds 2 minutes, or when the non-negative entries add up to more than 5 minutes. The array is
cleared after each alert.

## Other signals

- **Stopping monitoring from the app** (`Daemon::note_user_stop`, reached over IPC as
  `NoteUserStop` on Linux and macOS) sends a `UserStop` event at `EXTRA_HIGH_RISK` straight away
  (CORE-011). It also excuses the next session's first late-wakeup check, so restarting isn't
  reported a second time. A plain kill or termination signal gets no such excuse.
- **Repeated restarts** (CORE-019): 20 daemon restarts within 10 minutes send a
  `repeated_restarts` event, at most once every 30 minutes. This catches a daemon that is killed
  and restarted faster than the late-wakeup check can see.
- **Capture failures** (CORE-006): five failed screenshots within 30 minutes send a
  `capture_failed` event at 80% risk, which is what a revoked screen-recording permission
  looks like.
- **Clock changes** (CORE-027): the core keeps its own clock from the server's `Date` header,
  carried forward on the boot clock, so setting the system clock changes nothing it records.
  When a server response shows the core's clock was more than 5 minutes off, it sends an
  `alert` event saying by how much. That is what catches a restart hidden by setting the clock
  back.

iOS turns off the late-wakeup, restart and clock checks (`lifecycle_enabled()` is `false`). Its
monitoring runs in a Safari extension that the OS suspends without notice, so every gap looks
the same and none of them means tampering. Stopping from the app and capture failures are
still reported there.

## Known limits

These are accepted under the current threat model (see the non-goals in `docs/overview.md`):

- **Quitting and never restarting** produces no next tick, so the core can't report it. Partners
  see it as a device that stops sending logs: it shows as offline, and their digest names it.
- **Uninstalling** sends nothing, for the same reason.
- **Client-stored state is trusted.** Someone who edits `event_state.json` can hide a gap. The
  core makes no attempt to detect that.
- **A late wakeup has no cause attached.** Apart from the excuses above, a crash, a slow boot and
  a force-kill before logout all look the same: a gap against the schedule.
