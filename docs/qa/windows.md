# Windows QA Checklist

**Test environment**

- Windows version (winver):
- App version (from About or MSIX manifest):
- API environment (prod / staging / local):
- Tester:
- Date:

---

## 1. Installation

- [ ] MSIX installs without error (signed cert trusted, or dev cert helper used for sideloading)
- [ ] After install, `VirtueTrayStartup` startup task is present in Task Manager → Startup apps
- [ ] Launching Virtue from Start creates the tray icon without opening a window
- [ ] App appears in installed apps list (**Settings → Apps**)

---

## 2. First launch

- [ ] Tray icon appears in the system tray (notification area) on first launch
- [ ] Double-clicking tray icon or clicking **Open** opens the settings / login window
- [ ] Window closes back to tray when dismissed (does not exit the app)
- [ ] No UAC prompt required for normal operation (monitoring runs in user session)

---

## 3. Login

- [ ] Login window accepts email + password
- [ ] Submitting invalid credentials shows an error; app does not crash
- [ ] Successful login:
  - [ ] `%PROGRAMDATA%\Virtue\config\token_store.json` is written
  - [ ] Device is registered with the API
  - [ ] Settings window updates to show signed-in state
- [ ] Monitoring starts automatically after login (resident monitoring host activates)
- [ ] `%PROGRAMDATA%\Virtue\data\service.log` shows monitoring active

---

## 4. Monitoring starts after login

- [ ] `%PROGRAMDATA%\Virtue\data\audit.jsonl` gains entries after one capture interval
- [ ] Settings window shows monitoring as active
- [ ] Tray icon tooltip or context menu reflects active monitoring

---

## 5. Screenshot capture and upload

- [ ] Screenshots captured at configured interval (default 300 s)
- [ ] Screenshots are blurred, resized (smaller dimension ≤ 128 px), WebP-encoded
- [ ] Batch upload fires at configured batch interval (default 3600 s)
- [ ] Web app shows received batches for this device
- [ ] Web app can decrypt and display screenshots
- [ ] `POST /hash` called once per screenshot; hash chain appears in web app

---

## 6. Lifecycle logs

- [ ] App sends `daemon_start` log on resident monitoring host start
- [ ] `system_startup` log sent on machine boot (verify after full reboot)
- [ ] `system_shutdown` log sent on normal shutdown (trigger via Start → Shut down)
- [ ] Tray **Exit** (after confirmation) sends user-stop log before app exits
- [ ] Lifecycle events visible in web app device activity

---

## 7. Tray and UI actions

- [ ] Right-click tray icon shows context menu with at minimum **Open** and **Exit**
- [ ] **Exit**: prompts for confirmation, records explicit user stop, stops resident monitoring, exits app
  - Does not log the device out (auth state preserved)
- [ ] Re-launching from Start reuses the running resident instance (no duplicate process)
- [ ] Settings window can be reopened after being hidden
- [ ] Login / logout UI state matches actual auth state on reopen

---

## 8. Auto-start behavior

- [ ] After reboot, Virtue starts automatically via the startup task
- [ ] No second tray icon appears if app is already running when clicked in Start
- [ ] Disabling the startup task in Task Manager prevents auto-start; re-launching manually re-enables it

---

## 9. Resident monitoring host (FFI)

- [ ] `virtue_windows_get_monitor_status_json` returns non-error status after login
- [ ] `virtue_windows_get_session_status_json` returns expected session info
- [ ] `virtue_windows_stop_monitoring` + `virtue_windows_start_monitoring` cycle works without crash
- [ ] `virtue_windows_set_runtime_config_json` applies config immediately; next loop uses new values

---

## 10. Token refresh

- [ ] With a near-expired device access token, upload triggers `POST /d/token` automatically
- [ ] Monitoring continues after token refresh; no user action required

---

## 11. Runtime config override

- [ ] Edit `%PROGRAMDATA%\Virtue\config\config.json`:
  ```json
  { "apiBaseUrl": "http://localhost:8787", "captureIntervalSeconds": 15, "batchWindowSeconds": 30 }
  ```
- [ ] Change is picked up on the next loop iteration without restarting the app
- [ ] Settings window reflects updated config values if displayed
- [ ] Removing the override keys reverts to defaults

---

## 12. Logout

- [ ] Logging out from settings window clears `token_store.json`
- [ ] Monitoring stops after logout
- [ ] Re-login creates a new session and resumes monitoring

---

## 13. Uninstall

- [ ] Uninstalling via **Settings → Apps** removes the startup task
- [ ] Tray icon disappears after uninstall completes (or after reboot)
- [ ] `%PROGRAMDATA%\Virtue\` data directory is NOT removed by uninstall (expected — user data)
- [ ] Reinstalling works cleanly over leftover data directory

---

## 14. Edge cases

- [ ] No internet: captures queue in `audit.jsonl`; uploads retry when connectivity returns
- [ ] API 401: daemon calls `POST /d/token`, retries once; if refresh fails, monitoring idles and logs error
- [ ] API 400: permanent failure written to `service.log`, not retried
- [ ] Disk write failure: errors logged, app does not crash
- [ ] Windows UAC / multiple user accounts: each user session has its own `%PROGRAMDATA%\Virtue\` sub-path (or confirm shared path behavior)
- [ ] App installed on a machine where another user is signed into Windows: verify no cross-user data leakage
- [ ] Screen locked / Windows locked workstation: monitoring should continue (service runs in user session); verify captures resume after unlock
- [ ] Hibernation: verify monitoring resumes after resume from hibernate without re-login

---

## 15. Version upgrade

- [ ] Installing a newer MSIX over an existing installation completes cleanly
- [ ] Existing `audit.jsonl` entries are replayed after upgrade
- [ ] No errors in `service.log` after upgrade
- [ ] Startup task still present after upgrade
