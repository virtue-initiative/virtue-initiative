# Linux QA Checklist

**Test environment**

- Distro + version:
- Desktop environment (GNOME / KDE / Sway / none):
- Display server (X11 / Wayland):
- App version (`virtue --version`):
- API environment (prod / staging / local):
- Tester:
- Date:

---

## 1. Installation

- [ ] `.deb` installs without errors:
  ```bash
  sudo dpkg -i virtue-<version>.deb
  sudo apt-get install -f  # resolve any missing deps
  ```
- [ ] `virtue` binary is in `$PATH`:
  ```bash
  which virtue
  virtue --version
  ```
- [ ] `postinst` script installs and enables the user systemd unit:
  ```bash
  systemctl --user status virtue.service
  ```
- [ ] Service starts automatically for a desktop session after install

---

## 2. Login

- [ ] `virtue login` prompts for email and password
- [ ] Invalid credentials print a clear error; CLI exits non-zero
- [ ] Valid credentials:
  - [ ] Registers device with API
  - [ ] Writes `auth.json` under `~/.local/state/virtue/` (or `$XDG_STATE_HOME/virtue/`)
  - [ ] Writes `device_settings.json`
  - [ ] Prints confirmation message
- [ ] `virtue status` shows logged-in state after login
- [ ] Login triggers a capture probe; probe failure prints a useful error (e.g., missing capture tool on X11)
- [ ] Login with already-registered device name succeeds

---

## 3. Capture probe (login-time)

**X11:**
- [ ] `imagemagick` (`import`) or `maim` is present; capture probe succeeds
- [ ] If neither is installed, probe fails with a clear instruction message

**Wayland:**
- [ ] Capture probe either succeeds with `grim` (if configured) or reports that unattended capture is not supported
- [ ] Monitoring continues (scheduling/upload works) even if capture probe returns a warning

---

## 4. Monitoring lifecycle via systemd

- [ ] `virtue daemon start` starts `virtue.service` via `systemctl --user start`
  ```bash
  systemctl --user status virtue.service
  ```
- [ ] `virtue daemon stop` stops the service and records a user-stop-intent log:
  ```bash
  virtue daemon stop
  systemctl --user status virtue.service  # should show inactive
  ```
- [ ] `virtue daemon` (run directly) starts the background capture + upload loop
- [ ] Tray icon appears on desktop sessions when daemon is running (skip on headless)
- [ ] Tray icon hover shows current status

---

## 5. Screenshot capture and upload

- [ ] Screenshots captured at configured interval (default 300 s; minimum 15 s)
- [ ] Screenshots are blurred, resized (smaller dimension ≤ 128 px), WebP-encoded
- [ ] `audit.jsonl` gains new entries after one capture interval:
  ```bash
  tail ~/.local/state/virtue/audit.jsonl
  ```
- [ ] Batch upload fires at configured batch interval (default 3600 s; minimum 1 s)
- [ ] Web app shows received batches for this device
- [ ] Web app can decrypt and display screenshots
- [ ] `POST /hash` called once per screenshot; hash chain visible in web app

---

## 6. Lifecycle logs

- [ ] `daemon_start` log sent on service start
- [ ] `daemon_stop_signal` log sent when service stops
- [ ] `system_startup` log sent on next daemon start after a reboot (boot-id change detected via `/proc/sys/kernel/random/boot_id`)
- [ ] `system_shutdown` log sent when service stops while systemd is in `stopping` state (trigger via `sudo shutdown now`)
- [ ] All lifecycle events visible in web app device activity

---

## 7. `virtue status`

- [ ] Shows current login state (logged in / out)
- [ ] Shows monitoring state (active / idle)
- [ ] Shows current `api_base_url`, `capture_interval_seconds`, `batch_window_seconds`
- [ ] Reflects values from `config.json` override when present

---

## 8. Developer commands (optional / internal)

- [ ] `virtue dev upload-log --risk 0.7` sends a log immediately; visible in web app
- [ ] `virtue dev add-log --risk 0.7` queues a metadata-only log into the next batch
- [ ] `virtue dev add-screenshot --risk 0.7` captures and queues a screenshot into the next batch
- [ ] `virtue dev upload-batch` forces an immediate batch upload; web app shows new batch

---

## 9. Token refresh

- [ ] With a near-expired device access token, the daemon calls `POST /d/token` automatically
- [ ] Monitoring continues uninterrupted after token refresh

---

## 10. Runtime config override

- [ ] Create `~/.config/virtue/config.json` (or `$XDG_CONFIG_HOME/virtue/config.json`):
  ```json
  { "api_base_url": "http://localhost:8787", "capture_interval_seconds": 15, "batch_window_seconds": 30 }
  ```
- [ ] Override is applied on the next loop iteration without restarting the daemon
- [ ] `virtue status` reflects the new values
- [ ] Removing the config file reverts to defaults on the next loop iteration

---

## 11. XDG paths

- [ ] With `XDG_CONFIG_HOME=/tmp/test-config`, `virtue` reads config from `/tmp/test-config/virtue/config.json`
- [ ] With `XDG_STATE_HOME=/tmp/test-state`, state files are written under `/tmp/test-state/virtue/`

---

## 12. Logout

- [ ] `virtue logout` warns that a log is sent, then clears auth state
- [ ] After logout, `virtue status` shows logged out
- [ ] Daemon idles after logout (no captures, no uploads)
- [ ] `auth.json` is removed or cleared after logout
- [ ] Re-running `virtue login` creates a new session and resumes monitoring

---

## 13. Edge cases

- [ ] No internet: captures queue in `audit.jsonl`; uploads retry when connectivity returns
- [ ] API 401: daemon calls `POST /d/token`, retries once; if refresh fails, idles and logs to `errors.log`
- [ ] API 400: permanent failure written to `errors.log`, not retried
- [ ] Disk full / write failure: errors logged, daemon does not crash or spin
- [ ] Non-systemd distro: monitoring daemon can still be run manually; lifecycle logs (`system_startup` / `system_shutdown`) are skipped gracefully
- [ ] Missing `/proc/sys/kernel/random/boot_id`: startup detection skips gracefully, no crash
- [ ] Wayland without `grim`: capture fails gracefully on each loop; other audit work (retries, uploads) continues

---

## 14. Uninstall / purge

- [ ] `sudo dpkg --remove virtue` removes the binary and disables the service unit
- [ ] `sudo dpkg --purge virtue` also removes config/state files (or verify that purge vs remove behavior is documented)
- [ ] After purge, `virtue` command is not found in `$PATH`

---

## 15. Version upgrade

- [ ] Installing a newer `.deb` over existing installation (`dpkg -i`) completes cleanly
- [ ] Service restarts with new binary; existing `audit.jsonl` is replayed
- [ ] No errors in `errors.log` after upgrade
