# Android QA Checklist

**Test environment**

- Device / emulator model:
- Android version (API level):
- App version (from About or `adb shell pm dump org.virtueinitiative.virtue | grep versionName`):
- API environment (prod / staging / local):
- Tester:
- Date:

---

## 1. Installation

- [ ] APK / release build installs without error
  ```bash
  adb install -r app-release.apk
  ```
- [ ] App appears in the launcher
- [ ] No immediate crash on launch

---

## 2. First launch and login

- [ ] Login screen (email + password) is displayed on first launch
- [ ] Submitting invalid credentials shows an error message; app does not crash
- [ ] Successful login:
  - [ ] Registers device with API (`POST /d/device`)
  - [ ] Prompts for **MediaProjection** capture permission (screen capture consent dialog)
  - [ ] After granting permission, foreground monitoring service starts
  - [ ] A persistent notification appears indicating monitoring is active

---

## 3. MediaProjection permission

- [ ] Denying MediaProjection permission: app handles gracefully (shows error or retries on next start); no crash
- [ ] Granting permission: monitoring service starts and captures begin
- [ ] On some OEM devices / OS versions, MediaProjection permission may require re-grant after reboot — verify behavior and document if re-grant is required
- [ ] Revoking permission from Settings mid-session: capture failures are logged; service continues running (for retries / upload of queued data)

---

## 4. Foreground monitoring service

- [ ] `adb shell pidof -s org.virtueinitiative.virtue` shows a running process
- [ ] Persistent notification is shown with monitoring status
- [ ] Service is marked `START_STICKY` — pulling it from recent apps restarts it via `AlarmManager`
  - Close app from Recents; verify service restarts within ~1 minute
- [ ] `WorkManager` keepalive task fires periodically; service continues after device idle

---

## 5. Screenshot capture and upload

- [ ] Screenshots captured at configured interval (default from core; override-able)
- [ ] Screenshots are blurred, resized (smaller dimension ≤ 128 px), WebP-encoded
- [ ] Batch upload fires at configured batch interval
- [ ] Web app shows received batches for this device
- [ ] Web app can decrypt and display screenshots
- [ ] `POST /hash` called per screenshot; hash chain visible in web app

---

## 6. Boot and package-replace receivers

- [ ] Reboot device while app is installed and logged in
  - [ ] After boot, monitoring service restarts automatically (boot receiver triggers)
  - [ ] No duplicate service instances
- [ ] Update/reinstall app (`adb install -r` over existing install)
  - [ ] Package-replaced receiver restarts the monitoring flow
  - [ ] No re-login required after update (auth state preserved)

---

## 7. Aggressive background survival

- [ ] Put device in Doze mode (leave idle for several minutes or `adb shell dumpsys deviceidle force-idle`)
  - [ ] Service remains running or restarts within the next keepalive window
- [ ] Battery optimization: verify app is excluded from battery optimization or behavior with optimization enabled is documented
  ```
  Settings → Battery → Battery optimization → Virtue → Don't optimize (recommended for QA)
  ```
- [ ] Force-stopping the app via Settings → Apps → Virtue → Force stop: service restarts via alarm within ~1 minute

---

## 8. Sign out

- [ ] Tapping sign out (from whatever UI is exposed):
  - [ ] Sends a log indicating monitoring was turned off
  - [ ] Clears auth / device state
  - [ ] Stops the foreground monitoring service
  - [ ] Persistent notification is dismissed
  - [ ] Returns to login UI

---

## 9. Token refresh

- [ ] With a near-expired device access token, the Rust core calls `POST /d/token` automatically
- [ ] Monitoring continues without user interaction after token refresh

---

## 10. Runtime config override

- [ ] Login screen exposes "Runtime overrides (optional)" fields:
  - `VIRTUE_BASE_API_URL`
  - `VIRTUE_CAPTURE_INTERVAL_SECONDS`
  - `VIRTUE_BATCH_WINDOW_SECONDS`
- [ ] Tapping **Save overrides** persists values and applies them to the Rust core immediately
- [ ] For emulator: use `http://10.0.2.2:8787` to reach host machine API (not `localhost`)
- [ ] Clearing overrides reverts to production defaults

---

## 11. Edge cases

- [ ] No internet: captures queue; uploads retry when connectivity returns
- [ ] API 401: core calls `POST /d/token`, retries once; if refresh fails, monitoring idles and logs error
- [ ] API 400: permanent failure logged, not retried
- [ ] MediaProjection virtual display: verify capture works at different screen orientations (portrait / landscape)
- [ ] Screen off: verify service continues running and captures are attempted (Android may reduce capture frequency when screen is off depending on power state)
- [ ] Multi-window / split-screen: verify no crash when app is in split-screen mode
- [ ] Low storage: write failures logged gracefully, service does not crash

---

## 12. Permissions audit

- [ ] App only requests the permissions listed in `AndroidManifest.xml`; no unexpected permission dialogs
- [ ] `FOREGROUND_SERVICE` and `FOREGROUND_SERVICE_MEDIA_PROJECTION` permissions present
- [ ] `RECEIVE_BOOT_COMPLETED` permission present (for boot receiver)
- [ ] `POST_NOTIFICATIONS` permission requested at runtime on Android 13+ (for persistent notification)

---

## 13. Version upgrade

- [ ] Installing a newer APK over the existing one (`adb install -r`) completes cleanly
- [ ] Monitoring resumes after update without re-login
- [ ] Queued audit data is preserved and replayed after upgrade
