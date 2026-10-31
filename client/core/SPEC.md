# Virtue Core

## CORE-001 Overview

The core MUST be structured around a single daemon loop. It's primary goal is to take screenshots (using the platform hooks), while detecting if a screenshot was missed for any reason.

The loop SHOULD be like this

```
loop(persisted state):
  state = check lifecycle(state)
  state = take screenshot(state)

  if late_wakeups.max() > 1000*60*2 OR late_wakeups.sum() > 1000*60*5:
    upload alert

  take screenshot
  process screenshot
  upload screenshot

  pick wakeup time
  return persisted state
  sleep until next time
```

## CORE-020 Wakeup scheduling

The wakeup time picked at the end of a tick MUST be the earliest time at which scheduled work can make progress: the next screenshot draw, the next heartbeat, the pending batch becoming eligible to upload (its batch interval elapsing, or immediately when it is otherwise ready to send, e.g. a forced flush), or a hash/batch retry backoff expiring. Pending work that is held by a condition no timer can clear (e.g. uploads held while the screen is locked, or no device credentials or settings) MUST NOT pull the wakeup time earlier; it waits for the next wakeup or client request instead.

The loop MUST NOT run a tick before the wakeup time unless a client request was applied. It MAY wake before the wakeup time (e.g. to bound how far a platform timer can drift across a system suspend), but MUST then go back to waiting without running a tick.

## CORE-002 Tamper detection

It SHOULD do tamper detection by comparing the current time to the expected wakeup time. The difference SHOULD be added to the late wakeups array unless the gap is explained by a legitimate session transition, checked from both ends:

- login evidence: the current time is within 2 minutes of the last system login
- logout evidence: the expected wakeup time was within 2 minutes of the last system logout

Each piece of evidence is either unavailable (the platform hook returned nothing), supporting (within its 2-minute window), or contradicting (available but outside the window). A timestamp outside its window is contradicting only when the other timestamp is supporting; on its own, or beside another out-of-window timestamp, it is just a stale record (e.g. a login from days ago) and counts as unavailable. The gap MUST be excused (skipped, not added to the late wakeups array) only if at least one piece of evidence is supporting and neither piece is contradicting. A single contradicting signal MUST block the excuse even if the other signal is supporting — this is what keeps a daemon that was killed well before an eventual reboot (stale expected-wakeup vs. the reboot's logout) or one that isn't restarted until long after the surrounding login (e.g. autostart disabled) from riding a login/logout event it wasn't actually bracketed by.

A third piece of evidence — suspend evidence — MUST also be considered, to excuse a gap caused by the system being suspended rather than by tampering. It is computed by comparing the real-time elapsed since the previous tick against the elapsed time on a clock that does not advance while the system is suspended, over that same span; a shortfall reveals how much of the span the system spent suspended. Unlike login/logout evidence, suspend evidence MUST only ever support an excuse, never contradict one — it MUST NOT be able to block a login/logout-supported excuse. It MUST support the excuse only when the detected suspended duration accounts for essentially the whole gap (allowing a small amount of slack for wake-up scheduling jitter); a gap only partially covered by a brief suspend (e.g. a killed process whose downtime happens to also span a real suspend) MUST NOT be excused by this evidence.

The late wakeups array SHOULD track the last 10 wakeups. If a single late wakeup is greater than 2 minutes or the sum (of the non-negative values) is greater than 5 minutes we SHOULD alert.

The late wakeups array MUST be cleared after an alert is sent (to prevent duplicates).

The late wakeup event SHOULD be called "screenshot_missed".

Tamper detection MUST be skipped, recording nothing, on a platform whose hooks report `lifecycle_enabled` as false (iOS, which has no boot or session signal to compare against).

A user-initiated stop MUST excuse the gap that follows it — the lateness check MUST be skipped once, rather than recorded, so restarting after it isn't also reported as tampering on top of the user-stop alert. This skip MUST apply to the first tick of the monitoring session that follows the daemon actually stopping and restarting, not to any tick that runs beforehand in the same still-running session (e.g. while an already-requested stop is still being processed) — a tick that happens before the daemon has actually stopped isn't the gap being excused, and consuming the excuse there would leave the real gap unprotected. A stop MUST NOT be excused this way merely because the process exited cleanly (e.g. a caught termination signal) — only an actual user-initiated stop, since anything broader would let simply killing the process defeat tamper detection.

## CORE-003 Screenshots

Screenshots MUST be captured randomly (i.e. every second there is the same chance as any other second) about every 5 minutes. The first screenshot after login MUST be taken on the next tick. Nothing is captured while logged out.

A capture that comes due while the screen is locked or a screensaver is active MUST be skipped: the daemon MUST send a `screenshot_skipped` event (reason `locked_or_screensaver`, BATCH-003) and draw the next capture time as usual.

A capture that has not materially changed from the last uploaded frame MUST NOT be uploaded; the daemon MUST send a `screenshot_skipped` event (reason `static_screen`) instead. The comparison MUST be against the last uploaded frame, not the previous capture, so that gradual change eventually counts. A broad change, even a low-contrast one such as a terminal filling with text, and a change concentrated in one region, such as a video, MUST count as changed. A few isolated cells changing, such as a clock, MUST NOT. The grid compared MUST scale with the screen size, and a change of screen size MUST count as changed.

A capture or image processing failure MUST NOT be uploaded as a screenshot. It MUST be retained as in CORE-018 and counted towards CORE-006's `capture_failed` event.

### CORE-004 Content detection

Screenshots SHOULD be processed with the content detection logic.

Screenshots SHOULD first be filtered based on a skin tone detector. If it rates it above a threashold, it SHOULD run a NSFW image classifier.

When the `per_image_nsfw` feature flag is on (see /api/SPEC.md API-061), the daemon SHOULD instead find the photo-like regions of the screenshot and run the classifier on each region that passes its own skin tone filter, then combine the region scores into one risk. A grayscale region MAY skip the skin tone filter, since the detector can't see skin without color. When no region is found, the whole screenshot SHOULD be scored the same way, as a single region. The skin tone detector used for this SHOULD recognize dark skin tones. Phone screenshots MAY be scored differently from desktop ones, since a desktop can show several windows. When the flag is off, the daemon MUST score screenshots as described above.

Screenshots SHOULD then be redacted using an OCR engine and then compressed into small WebP.

The skin tone score MUST be 0 for an image with no skin-toned pixels (e.g. all white or all black) and 1 for an image that is entirely skin-toned. The WebP MUST be scaled down by the same fixed factor on each axis (rounding up, never below one pixel), so that each monitor keeps the same detail however the monitors are laid out.

## CORE-005 Uploading

Requests SHOULD be queued to be uploaded in a batch.

See /api/SPEC.md and /hash-server/SPEC.md for details on the format.

Both hash server uploads and batch uploads MUST be stored in the state object and retried with exponentional backoff on each wakeup. A batch that cannot be built MUST back off in the same way rather than be retried immediately.

While no device credentials are stored, events MUST NOT be queued.

A batch MUST NOT be uploaded while the screen is locked or a screensaver is active, unless an upload was forced by a heartbeat or `flush_batch_now`.

An event with a risk of 90% or more, rounded to a whole percent, MUST force the next batch to upload without waiting for the batch interval. A `heartbeat` MUST do the same and MUST also bypass the screen-locked hold. In the batch's `event_counts` (API-038), an event MUST count as `high` at 70% or more and as `medium` from 40% to below 70%, using the same rounded percentage, so that the counts agree with the labels the web app shows.

A batch MUST hold every event hashed since the last batch, in the order they were hashed (BATCH-006). Once 200 hashed events are waiting for a batch, the daemon MUST NOT upload more hashes until that batch has uploaded.

> Note: device settings SHOULD be refreshed on process startup and on batch upload and saved to the state object

The daemon MUST save `feature_flags` from the device settings with the rest of the settings, and MUST treat every flag as off until settings have been fetched (see /api/SPEC.md API-061).

The hash server token returned with the device settings is valid for 2 hours (see /api/SPEC.md API-002). The daemon SHOULD keep using it until shortly before it expires, and MUST NOT refresh it on a schedule shorter than the batch interval. If the hash server rejects the token as unauthorized, the daemon MUST fetch a new one before its next hash upload.

## CORE-006 Other events

When the daemon detects that the System Login time changed, it MUST send a "system login at" event (risk 0%).

When the daemon detects thtat hte System Logout time changed, it MUST send a "system logout at" event (risk 0%).

While authenticated, the daemon MUST send a `heartbeat` event (risk 0%) at least once every 24 hours.

When five screenshot captures fail within 30 minutes, the daemon MUST send a `capture_failed` event (risk 50%) and reset the failure count. A failure older than 30 minutes MUST NOT count.

The first System Login/Logout time observed (i.e. there is no prior value to compare against) MUST NOT be reported — it only establishes the baseline a later change is measured against.

## CORE-007 Client API

The core MUST expose the following methods to clients. Each blocks until the loop thread has applied and persisted the request, except `status`, which reads the last-committed state directly, and `request_stop`, which is fire-and-forget.

### CORE-008 login

`login(email, password, device_name?) -> device_id`

MUST register a new device with the API before anything else. On failure MUST leave the persisted state unchanged, including any existing session, its queued events and screenshot capture. On success, it SHOULD try once to upload the pending batch with any existing session, and a failure of that upload MUST NOT fail the login. It MUST then log out (CORE-009), so that queued events still unsent are discarded and the new session shares no state with the old one. It MUST then store the returned device credentials, settings, and hash token, and MUST enable screenshot capture. `device_name` is optional; if absent or blank, the platform's configured default name MUST be used.

### CORE-009 logout

`logout()`

MUST best-effort revoke the device session with the API, and MUST reset all persisted state to its initial value, which clears stored device credentials and queued events and disables screenshot capture. A logout the daemon performs because the server rejected the device MUST keep `recent_errors` (CORE-018), so that the reason stays visible. A batch upload that was in flight when the device logged out MUST have its result discarded, so that it cannot restore queued events.

### CORE-010 status

```
status() -> {
  is_authenticated, is_running,
  account_email, device_id, device_name, partner_count,
  pending_hash_count, pending_batch_count, pending_request_count,
  last_loop_at_ms, last_screenshot_attempt_at_ms, last_screenshot_at_ms,
  last_skip_reason, last_batch_at_ms,
  recent_errors,
  api_base_url, hash_base_url, capture_interval_seconds, batch_window_seconds
}
```

MUST return, without blocking on the loop:

- `is_authenticated` / `is_running` — whether device credentials are stored, and whether the loop is running.
- `account_email`, `device_id`, `device_name` — the account and device this install is registered as, where known.
- `partner_count` — the number of partners this device wraps batch keys for: the number of wrapping keys minus the owner's own key. MUST be absent when device settings have never been fetched, which is distinct from a count of zero.
- `pending_hash_count` / `pending_batch_count` — the number of events waiting to be uploaded to the hash server, and the number waiting to go out in a batch. `pending_request_count` is retained as the coarser combined figure.
- `last_loop_at_ms` — the timestamp of the last completed tick (if any).
- `last_screenshot_attempt_at_ms`, `last_screenshot_at_ms`, `last_skip_reason` — see CORE-018.
- `last_batch_at_ms` — when a batch last uploaded successfully.
- `recent_errors` — see CORE-018.
- `api_base_url`, `hash_base_url`, `capture_interval_seconds`, `batch_window_seconds` — the effective configuration this build is running with, for the diagnostics/advanced section of a client's status page.

Every field except `is_running` MUST be derivable from persisted state plus the compile-time configuration, so a client whose daemon process is not running can compute the same status from `state_path` alone and report only `is_running: false`.

### CORE-011 note_user_stop

`note_user_stop(source)`

MUST record that the user explicitly stopped monitoring while it was expected to be running (see §2's `user_stop` alert).

### CORE-022 note_user_start

`note_user_start()`

MUST record that monitoring has resumed after a prior `note_user_stop`, so tamper detection (CORE-002) resumes, and MUST send a `user_start` event. A no-op, sending nothing, when no stop is active. The core MUST also offer `queue_note_user_start()`, a variant that only enqueues this request and returns without waiting for the loop's reply, which the loop applies before anything queued after it. A caller that is itself about to start the loop (Android's accessibility service restarting it) MUST use that variant: the blocking form waits on a loop that is not yet running and fails with a timeout, and anything the caller queues behind it (such as `login`) waits out the same timeout.

### CORE-012 queue_upload

`queue_upload(upload)`

MUST enqueue the given event directly for hashing and batch upload, bypassing the daemon's own capture/lifecycle logic.

### CORE-013 flush_batch_now

`flush_batch_now()`

MUST force the next tick to upload the current batch immediately rather than waiting for the batch interval.

### CORE-014 request_stop

`request_stop()`

MUST stop the loop after its current tick. The loop MAY be started again afterward. The request stays queued until a loop consumes it, so a caller MUST NOT send it while no loop is running: the next loop to start would stop immediately.

### CORE-015 tick_once

`tick_once()`

MUST apply and persist any currently-queued requests, then MUST run exactly one tick, then MUST return — without waiting for a scheduled wakeup and without looping. For a platform with no way to keep a background thread alive between invocations (iOS's Safari-extension native message handler, which the OS only guarantees runs for the duration of one request/response round trip — see `architecture.md`), this MUST be the method called once per invocation instead of `run_forever`. MUST NOT be called concurrently with `run_forever` or with itself on the same `Daemon`.

### CORE-021 force_capture_now

`force_capture_now()`

MUST take a screenshot on the next tick even if one is not yet due, still honoring the locked/screensaver gate, and MUST then force the batch holding it to upload as `flush_batch_now` does.

A platform that drives the daemon with `tick_once` has no loop to service this request while the caller waits for it. It MUST instead be able to ask for a single `tick_once` that applies a `force_capture_now` before the tick, as if that request had been queued.

## CORE-016 State persistence

State MUST be persisted to a single JSON file (`state_path`) via a tmp-file-plus-rename so a reader never observes a partially-written file.

On platforms where more than one OS process can construct a `Daemon` against the same `state_path` concurrently (iOS: the Safari extension's own daemon and the app's on-demand daemon started to service a blocking client call), each read-modify-write of that state — whether applying a client request or running a tick — MUST be serialized against other processes by holding an OS-level advisory exclusive lock on a sibling lock file (`state_path` with its extension replaced by `.lock`) for the full span from re-reading state through persisting the result. The lock MUST be released between iterations/requests rather than held for a `Daemon`'s whole lifetime, so a process not currently mutating state cannot starve another process's access.

Because a `Daemon`'s working copy is normally cached in memory across iterations to avoid re-reading disk on every tick, a process MUST re-read `state_path` from disk immediately after acquiring the lock and before applying any mutation, rather than trusting its in-memory cache — otherwise its eventual persist would silently discard whatever another process wrote since this process's last read.

This locking MUST NOT be required — though it MAY be applied unconditionally, since it is then always uncontended — on platforms where at most one process ever holds a `Daemon` for a given `state_path` (Linux/Mac/Windows, which route all other processes' access through `ipc.rs`'s socket instead of a second `Daemon`; Android's single-process model).

## CORE-017 Unreadable state recovery

If `state_path` exists but its contents cannot be parsed into the expected state type (truncated by a crash, corrupted, or written by an incompatible older/newer build), the reader MUST treat this the same as a missing file — falling back to that type's default value — rather than treating it as a fatal error. This MUST be logged (at error level) so the condition is diagnosable, but MUST NOT prevent `Daemon::new` from returning successfully. Losing an unreadable file's contents is an acceptable trade-off against the alternative: a `Daemon` that fails to construct leaves every caller permanently unable to distinguish "not authenticated" from "state unreadable," including on platforms whose UI has no path to report or recover from a `Daemon::new` failure short of clearing the file manually or reinstalling.

Before falling back to the default value, the reader MUST copy the unparseable file's original bytes, unmodified, to a sibling path so they remain available for debugging — otherwise the next persisted write silently overwrites the only evidence of what went wrong. A best-effort failure to write that backup (e.g. a read-only volume) MUST be logged but MUST NOT itself prevent the fallback to the default value.

## CORE-018 Retained status data

For a client's status page to be useful, the daemon MUST retain the following in persisted state rather than only emitting it as an upload event or a log line:

- `last_screenshot_attempt_at_ms` — the time of the most recent tick at which a screenshot was due and acted on, whether it resulted in an upload, a skip, or a failure.
- `last_screenshot_at_ms` — the time of the most recent screenshot that was actually captured and enqueued for upload.
- `last_skip_reason` — why the most recent attempt did not produce a screenshot (see CORE-003's gates, plus capture failure). It MUST be cleared as soon as an attempt succeeds, so a stale reason cannot outlive the condition that caused it.
- `recent_errors` — a bounded, newest-first ring of the most recent errors the daemon hit (capture, hash upload, batch upload, settings refresh, state persistence), each with the time it occurred, a short stable context identifier, and the error's message. The ring MUST be capped (20 entries) so state cannot grow without bound, and MUST survive a restart, since a client whose daemon has just crashed and restarted is exactly when this is worth reading.

## CORE-019 Repeated restart detection

The daemon SHOULD track the timestamps of its own restarts, pruned to a
rolling 10-minute window, and SHOULD alert once 20 restarts fall within that
window, regardless of user-stop status. Alerts MUST be rate-limited to at
most one per 30 minutes. The restart-timestamps array MUST be cleared once
the threshold is reached, alert sent or not. The event SHOULD be called
"repeated_restarts". This check MUST be skipped where `lifecycle_enabled` is
false (see CORE-002).

## CORE-023 Logging

The core MUST NOT install a logging subscriber itself; each platform does. The default log filter MUST be `info`, with `virtue_core` at `debug`, in release builds as well as debug builds.

A platform with a rolling file log MUST keep only the 14 most recently modified files whose names start with the log prefix, and MUST NOT delete other files in the directory.

The log excerpt attached to a bug report (API-042) MUST be built from file modified times, not file names. It MUST contain every log file modified in the last 24 hours, oldest first, or the newest file alone when none is that recent. Opaque session tokens and JWT-shaped strings in it MUST be replaced with `[redacted]`, and other text MUST be left unchanged. It MUST be absent when there are no log files or they are all empty.

## CORE-024 Configuration

The API base URL MUST be stored with surrounding whitespace and any trailing slashes removed.

## CORE-025 Cross-process IPC

On Linux and macOS, the CLI and tray run as a separate process from the daemon. The daemon MUST serve the CORE-007 methods to them over a Unix socket, using one JSON message per line, and MUST serve one client at a time: once a client disconnects, the next MUST be served. If the daemon replies to `login` or `logout` with a failure that has an empty message, the caller MUST receive the message `login failed` or `logout failed` respectively.

## CORE-026 API error messages

When the API replies with a non-2xx status, the error reported to the caller MUST be, in order of preference: the validation messages in the body's `details`, the body's non-empty `error` field, the body text, or, when the body is missing or empty, a non-empty message naming the HTTP status code and its reason phrase (e.g. `HTTP 502 Bad Gateway error`).
