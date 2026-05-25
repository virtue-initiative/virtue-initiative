# macOS QA Checklist

**Test environment**

- macOS version:
- App version (`virtue --version` or from app bundle):
- API environment (prod / staging / local):
- Tester:
- Date:

---

## 1. Installation

- [ ] DMG mounts without Gatekeeper warning (or warning is expected for unsigned dev builds)
- [ ] Dragging Virtue.app to Applications completes without error
- [ ] App launches from Applications without crash
- [ ] First launch: LaunchAgent `org.virtueinitiative.virtue.daemon` is registered
  ```bash
  launchctl list | grep virtue
  ```
- [ ] Daemon binary starts within a few seconds of tray app launch
- [ ] Tray icon appears in the menu bar

---

## 2. Screen Recording permission

- [ ] On first launch (or after permission reset), macOS prompts for Screen Recording permission or the capture fails gracefully with a log entry
- [ ] Granting permission under **System Settings → Privacy & Security → Screen Recording** allows capture to proceed
- [ ] Revoking permission mid-session causes capture failures to appear in `errors.log`, not a crash
  ```bash
  cat ~/Library/Application\ Support/virtue/state/errors.log
  ```

---

## 3. Login

- [ ] Menu bar → **Open Virtue** when logged out shows email/password dialog
- [ ] Submitting invalid credentials shows an error message and does not crash
- [ ] Submitting valid credentials completes login:
  - [ ] `auth.json` is written under `~/Library/Application Support/virtue/state/`
  - [ ] `device_settings.json` is written in the same directory
  - [ ] Tray menu updates to show signed-in state
- [ ] Password is argon2id-hashed before being sent (verify no plaintext password in network traffic)
- [ ] Login with an already-registered device name succeeds (device re-registration)

---

## 4. Monitoring starts after login

- [ ] After login, daemon begins capturing screenshots at the configured interval
- [ ] `status.json` shows monitoring active
  ```bash
  cat ~/Library/Application\ Support/virtue/state/status.json
  ```
- [ ] `audit.jsonl` has new entries after one capture interval
  ```bash
  tail ~/Library/Application\ Support/virtue/state/audit.jsonl
  ```
- [ ] Tray icon tooltip or status reflects active monitoring

---

## 5. Screenshot capture and upload

- [ ] Screenshots are captured at approximately `screenshot_interval` seconds (default 300 s)
- [ ] Screenshots are blurred, resized (smaller dimension ≤ 128 px), and encoded as WebP before batching
- [ ] Batch upload (`POST /d/batch`) fires at approximately `batch_interval` seconds (default 3600 s)
- [ ] Web app shows received batches for this device after a batch upload cycle
- [ ] Batch is encrypted (wire format: 12-byte nonce + ciphertext); web app can decrypt and display screenshots
- [ ] `POST /hash` is called once per captured screenshot
- [ ] Hash chain appears in the web app for this device

---

## 6. Lifecycle logs

- [ ] `daemon_start` log is sent on daemon startup
- [ ] `system_startup` log is sent after a reboot (detects boot via `kern.boottime`)
- [ ] `system_shutdown` log is sent (best-effort) on normal shutdown / power off
  - Trigger via `sudo shutdown -h now` or menu → Shut Down
- [ ] `suspend` and `wake` logs are sent on sleep/wake cycle
  - Trigger via menu → Sleep or lid close
- [ ] Explicit **Stop Monitoring** via tray sends user-stop log before daemon exits
- [ ] Lifecycle log entries appear in the web app under device activity

---

## 7. Tray menu actions

- [ ] **Open Virtue** (logged in): shows signed-in state with **Stop Monitoring** and **Logout**
- [ ] **Stop Monitoring**: prompts for confirmation before stopping
  - After confirmation: tray icon exits, daemon stops, LaunchAgent is unregistered
  - Re-opening the app re-registers the LaunchAgent and resumes monitoring
- [ ] **Logout**: prompts for confirmation before logging out
  - After confirmation: auth state is cleared, LaunchAgent is unregistered, daemon stops
  - Re-opening shows login dialog

---

## 8. Daemon restart resilience

- [ ] Killing the daemon process (`kill <daemon-pid>`) causes it to restart via launchd
- [ ] After daemon restart, monitoring resumes without requiring re-login
- [ ] `audit.jsonl` retries any unresolved upload from before the crash
- [ ] No duplicate uploads for already-succeeded batches

---

## 9. Token refresh

- [ ] With a near-expired device access token, the next upload attempt calls `POST /d/token` to refresh
- [ ] Monitoring continues uninterrupted after token refresh

---

## 10. Runtime config override

- [ ] Create `~/Library/Application Support/virtue/config.json`:
  ```json
  { "api_base_url": "http://localhost:8787", "capture_interval_seconds": 15, "batch_window_seconds": 30 }
  ```
- [ ] Daemon picks up the override without restart (change takes effect on next `loop_iteration`)
- [ ] `virtue status` (if CLI is available) or `status.json` reflects the new intervals
- [ ] Removing `config.json` reverts to defaults

---

## 11. Logout and re-login

- [ ] After logout, `auth.json` is removed or cleared
- [ ] After logout, daemon idles (no captures, no uploads)
- [ ] Re-login creates a new device registration or reuses the existing one
- [ ] Monitoring resumes after re-login

---

## 12. Uninstall / clean state

- [ ] Dragging Virtue.app to Trash: LaunchAgent is NOT automatically removed (expected)
- [ ] Manually unloading the LaunchAgent stops the daemon:
  ```bash
  launchctl bootout gui/$(id -u)/org.virtueinitiative.virtue.daemon
  rm ~/Library/LaunchAgents/org.virtueinitiative.virtue.daemon.plist
  ```
- [ ] Removing `~/Library/Application Support/virtue/` clears all state

---

## 13. Edge cases

- [ ] No internet: captures are queued in `audit.jsonl`; uploads retry automatically when connectivity returns
- [ ] API returns 401: daemon calls `POST /d/token` and retries once; if refresh fails, daemon idles and logs error
- [ ] API returns 400: permanent failure is written to `errors.log`, not retried
- [ ] Disk full / write failure: errors appear in `errors.log`, daemon does not crash
- [ ] Multiple user accounts on same machine: each account's daemon uses its own `~/Library/Application Support/virtue/` directory
- [ ] Fast user switching: daemon for the switched-away user continues in the background
- [ ] macOS updates / SIP changes: verify Screen Recording permission is not silently revoked

---

## 14. Version upgrade

- [ ] Replacing Virtue.app with a newer version while the daemon is running: daemon continues or restarts cleanly
- [ ] After upgrade, existing `audit.jsonl` entries are replayed correctly
- [ ] No schema migration errors in `errors.log` after upgrade
