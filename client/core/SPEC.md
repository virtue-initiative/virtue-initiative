# Virtue Core

## 1. Overview

The core MUST be structured around a single daemon loop. It's primary goal is to take screenshots (using the platform hooks), while detecting if a screenshot was missed for any reason.

The loop SHOULD be like this

```
loop(persisted state):
  state = check lifecycle(state)
  state = take screenshot(state)

  if late_wakeups.max() > 1000*60 OR late_wakeups.sum() > 1000*60*5:
    upload alert

  take screenshot
  process screenshot
  upload screenshot

  pick wakeup time
  return persisted state
  sleep until next time
```

## 2. Tamper detection

It SHOULD do tamper detection by comparing the current time to the expected wakeup time. The difference SHOULD be added to the late wakeups array unless the gap is explained by a legitimate session transition, checked from both ends:

- login evidence: the current time is within 2 minutes of the last system login
- logout evidence: the expected wakeup time was within 2 minutes of the last system logout

Each piece of evidence is either unavailable (the platform hook returned nothing), supporting (within its 2-minute window), or contradicting (available but outside the window). The gap MUST be excused (skipped, not added to the late wakeups array) only if at least one piece of evidence is supporting and neither piece is contradicting. A single contradicting signal MUST block the excuse even if the other signal is supporting — this is what keeps a daemon that was killed well before an eventual reboot (stale expected-wakeup vs. the reboot's logout) or one that isn't restarted until long after the surrounding login (e.g. autostart disabled) from riding a login/logout event it wasn't actually bracketed by.

The late wakeups array SHOULD track the last 10 wakeups. If a single late wakeup is greater than 1 minute or the sum (of the non-negative values) is greater than 5 minutes we SHOULD alert.

The late wakeups array MUST be cleared after an alert is sent (to prevent duplicates).

The late wakeup event SHOULD be called "screenshot_missed".

## 3. Screenshots

Screenshots MUST be captured randomly (i.e. every second there is the same chance as any other second) about every 5 minutes.

### 3.1 Content detection

Screenshots SHOULD be processed with the content detection logic.

Screenshots SHOULD first be filtered based on a skin tone detector. If it rates it above a threashold, it SHOULD run a NSFW image classifier.

Screenshots SHOULD then be redacted using an OCR engine and then compressed into small WebP.

## 4. Uploading

Requests SHOULD be queued to be uploaded in a batch.

See /api/SPEC.md and /hash-server/SPEC.md for details on the format.

Both hash server uploads and batch uploads MUST be stored in the state object and retried with exponentional backoff on each wakeup.

> Note: device settings SHOULD be refreshed on process startup and on batch upload and saved to the state object

## 5. Other events

When the daemon detects that the System Login time changed, it MUST send a "system login at" event (risk 0%).

When the daemon detects thtat hte System Logout time changed, it MUST send a "system logout at" event (risk 0%).

## 6. Client API

The core MUST expose the following methods to clients. Each blocks until the loop thread has applied and persisted the request, except `status`, which reads the last-committed state directly, and `request_stop`, which is fire-and-forget.

### 6.1 login

`login(email, password, device_name?) -> device_id`

MUST revoke any existing device session first, then register a new device with the API. On success MUST store the returned device credentials, settings, and hash token, and MUST enable screenshot capture. On failure MUST leave the client logged out. `device_name` is optional; if absent or blank, the platform's configured default name MUST be used.

### 6.2 logout

`logout()`

MUST best-effort revoke the device session with the API, MUST clear stored device credentials, and MUST disable screenshot capture.

### 6.3 status

`status() -> { is_authenticated, is_running, device_id, last_loop_at_ms, pending_request_count }`

MUST return, without blocking on the loop: whether the user is authenticated, whether the loop is running, the device id (if any), the timestamp of the last completed tick (if any), and the number of requests currently queued.

### 6.4 note_user_stop

`note_user_stop(source)`

MUST record that the user explicitly stopped monitoring while it was expected to be running (see §2's `user_stop` alert).

### 6.5 queue_upload

`queue_upload(upload)`

MUST enqueue the given event directly for hashing and batch upload, bypassing the daemon's own capture/lifecycle logic.

### 6.6 flush_batch_now

`flush_batch_now()`

MUST force the next tick to upload the current batch immediately rather than waiting for the batch interval.

### 6.7 request_stop

`request_stop()`

MUST stop the loop after its current tick. The loop MAY be started again afterward.
