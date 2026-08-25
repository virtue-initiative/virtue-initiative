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

## CORE-002 Tamper detection

It SHOULD do tamper detection by comparing the current time to the expected wakeup time. The difference SHOULD be added to the late wakeups array unless the gap is explained by a legitimate session transition, checked from both ends:

- login evidence: the current time is within 2 minutes of the last system login
- logout evidence: the expected wakeup time was within 2 minutes of the last system logout

Each piece of evidence is either unavailable (the platform hook returned nothing), supporting (within its 2-minute window), or contradicting (available but outside the window). The gap MUST be excused (skipped, not added to the late wakeups array) only if at least one piece of evidence is supporting and neither piece is contradicting. A single contradicting signal MUST block the excuse even if the other signal is supporting — this is what keeps a daemon that was killed well before an eventual reboot (stale expected-wakeup vs. the reboot's logout) or one that isn't restarted until long after the surrounding login (e.g. autostart disabled) from riding a login/logout event it wasn't actually bracketed by.

A third piece of evidence — suspend evidence — MUST also be considered, to excuse a gap caused by the system being suspended rather than by tampering. It is computed by comparing the real-time elapsed since the previous tick against the elapsed time on a clock that does not advance while the system is suspended, over that same span; a shortfall reveals how much of the span the system spent suspended. Unlike login/logout evidence, suspend evidence MUST only ever support an excuse, never contradict one — it MUST NOT be able to block a login/logout-supported excuse. It MUST support the excuse only when the detected suspended duration accounts for essentially the whole gap (allowing a small amount of slack for wake-up scheduling jitter); a gap only partially covered by a brief suspend (e.g. a killed process whose downtime happens to also span a real suspend) MUST NOT be excused by this evidence.

The late wakeups array SHOULD track the last 10 wakeups. If a single late wakeup is greater than 2 minutes or the sum (of the non-negative values) is greater than 5 minutes we SHOULD alert.

The late wakeups array MUST be cleared after an alert is sent (to prevent duplicates).

The late wakeup event SHOULD be called "screenshot_missed".

A user-initiated stop MUST excuse the gap that follows it — the lateness check MUST be skipped once, rather than recorded, so restarting after it isn't also reported as tampering on top of the user-stop alert. This skip MUST apply to the first tick of the monitoring session that follows the daemon actually stopping and restarting, not to any tick that runs beforehand in the same still-running session (e.g. while an already-requested stop is still being processed) — a tick that happens before the daemon has actually stopped isn't the gap being excused, and consuming the excuse there would leave the real gap unprotected. A stop MUST NOT be excused this way merely because the process exited cleanly (e.g. a caught termination signal) — only an actual user-initiated stop, since anything broader would let simply killing the process defeat tamper detection.

## CORE-003 Screenshots

Screenshots MUST be captured randomly (i.e. every second there is the same chance as any other second) about every 5 minutes.

### CORE-004 Content detection

Screenshots SHOULD be processed with the content detection logic.

Screenshots SHOULD first be filtered based on a skin tone detector. If it rates it above a threashold, it SHOULD run a NSFW image classifier.

Screenshots SHOULD then be redacted using an OCR engine and then compressed into small WebP.

## CORE-005 Uploading

Requests SHOULD be queued to be uploaded in a batch.

See /api/SPEC.md and /hash-server/SPEC.md for details on the format.

Both hash server uploads and batch uploads MUST be stored in the state object and retried with exponentional backoff on each wakeup.

> Note: device settings SHOULD be refreshed on process startup and on batch upload and saved to the state object

## CORE-006 Other events

When the daemon detects that the System Login time changed, it MUST send a "system login at" event (risk 0%).

When the daemon detects thtat hte System Logout time changed, it MUST send a "system logout at" event (risk 0%).

The first System Login/Logout time observed (i.e. there is no prior value to compare against) MUST NOT be reported — it only establishes the baseline a later change is measured against.

## CORE-007 Client API

The core MUST expose the following methods to clients. Each blocks until the loop thread has applied and persisted the request, except `status`, which reads the last-committed state directly, and `request_stop`, which is fire-and-forget.

### CORE-008 login

`login(email, password, device_name?) -> device_id`

MUST revoke any existing device session first, then register a new device with the API. On success MUST store the returned device credentials, settings, and hash token, and MUST enable screenshot capture. On failure MUST leave the client logged out. `device_name` is optional; if absent or blank, the platform's configured default name MUST be used.

### CORE-009 logout

`logout()`

MUST best-effort revoke the device session with the API, MUST clear stored device credentials, and MUST disable screenshot capture.

### CORE-010 status

`status() -> { is_authenticated, is_running, device_id, last_loop_at_ms, pending_request_count }`

MUST return, without blocking on the loop: whether the user is authenticated, whether the loop is running, the device id (if any), the timestamp of the last completed tick (if any), and the number of requests currently queued.

### CORE-011 note_user_stop

`note_user_stop(source)`

MUST record that the user explicitly stopped monitoring while it was expected to be running (see §2's `user_stop` alert).

### CORE-012 queue_upload

`queue_upload(upload)`

MUST enqueue the given event directly for hashing and batch upload, bypassing the daemon's own capture/lifecycle logic.

### CORE-013 flush_batch_now

`flush_batch_now()`

MUST force the next tick to upload the current batch immediately rather than waiting for the batch interval.

### CORE-014 request_stop

`request_stop()`

MUST stop the loop after its current tick. The loop MAY be started again afterward.

### CORE-015 tick_once

`tick_once()`

MUST apply and persist any currently-queued requests, then MUST run exactly one tick, then MUST return — without waiting for a scheduled wakeup and without looping. For a platform with no way to keep a background thread alive between invocations (iOS's Safari-extension native message handler, which the OS only guarantees runs for the duration of one request/response round trip — see `architecture.md`), this MUST be the method called once per invocation instead of `run_forever`. MUST NOT be called concurrently with `run_forever` or with itself on the same `Daemon`.

## CORE-016 State persistence

State MUST be persisted to a single JSON file (`state_path`) via a tmp-file-plus-rename so a reader never observes a partially-written file.

On platforms where more than one OS process can construct a `Daemon` against the same `state_path` concurrently (iOS: the Safari extension's own daemon and the app's on-demand daemon started to service a blocking client call), each read-modify-write of that state — whether applying a client request or running a tick — MUST be serialized against other processes by holding an OS-level advisory exclusive lock on a sibling lock file (`state_path` with its extension replaced by `.lock`) for the full span from re-reading state through persisting the result. The lock MUST be released between iterations/requests rather than held for a `Daemon`'s whole lifetime, so a process not currently mutating state cannot starve another process's access.

Because a `Daemon`'s working copy is normally cached in memory across iterations to avoid re-reading disk on every tick, a process MUST re-read `state_path` from disk immediately after acquiring the lock and before applying any mutation, rather than trusting its in-memory cache — otherwise its eventual persist would silently discard whatever another process wrote since this process's last read.

This locking MUST NOT be required — though it MAY be applied unconditionally, since it is then always uncontended — on platforms where at most one process ever holds a `Daemon` for a given `state_path` (Linux/Mac/Windows, which route all other processes' access through `ipc.rs`'s socket instead of a second `Daemon`; Android's single-process model).

## CORE-017 Unreadable state recovery

If `state_path` exists but its contents cannot be parsed into the expected state type (truncated by a crash, corrupted, or written by an incompatible older/newer build), the reader MUST treat this the same as a missing file — falling back to that type's default value — rather than treating it as a fatal error. This MUST be logged (at error level) so the condition is diagnosable, but MUST NOT prevent `Daemon::new` from returning successfully. Losing an unreadable file's contents is an acceptable trade-off against the alternative: a `Daemon` that fails to construct leaves every caller permanently unable to distinguish "not authenticated" from "state unreadable," including on platforms whose UI has no path to report or recover from a `Daemon::new` failure short of clearing the file manually or reinstalling.

Before falling back to the default value, the reader MUST copy the unparseable file's original bytes, unmodified, to a sibling path so they remain available for debugging — otherwise the next persisted write silently overwrites the only evidence of what went wrong. A best-effort failure to write that backup (e.g. a read-only volume) MUST be logged but MUST NOT itself prevent the fallback to the default value.
