# iOS QA Checklist

**Test environment**

- Device model / iOS version (or simulator):
- App version (from Xcode or Settings → General → Virtue):
- API environment (prod / staging / local):
- Tester:
- Date:

> **Scope note:** The iOS client captures screenshots **only from Safari** via a Safari Web Extension. No system-wide or other-app capture occurs. All items below are scoped to Safari-only capture.

---

## 1. Installation and launch

- [ ] App installs to device or simulator without error
- [ ] App launches without crash
- [ ] Login / session / status UI is displayed on first launch
- [ ] No unexpected permission dialogs on first launch (Camera, Microphone, etc. should not appear)

---

## 2. Login

- [ ] Email + password fields accept input; login button is tappable
- [ ] Invalid credentials show an error message; app does not crash
- [ ] Successful login:
  - [ ] Registers device with API
  - [ ] Auth / state files are written to the shared App Group storage (`group.org.virtueinitiative.virtueios`)
  - [ ] UI transitions to signed-in state / session view
- [ ] UI correctly shows the logged-in user's email or name

---

## 3. Safari extension setup

- [ ] After login, UI prompts user to enable the extension in iOS Settings
- [ ] Navigate to **iOS Settings → Safari → Extensions → Virtue Safari Capture**
  - [ ] Extension appears in the list
  - [ ] Toggle on enables the extension
  - [ ] Permission scope set to **All Websites**
- [ ] Returning to the app: extension status indicator updates to reflect enabled state
- [ ] Disabling the extension: status indicator reflects disabled state; no crash

---

## 4. Capture via Safari

- [ ] Open Safari and browse to any website
- [ ] After one configured capture interval (default 15 s in dev/test builds), a screenshot is taken
- [ ] Only the visible Safari tab is captured; other apps are not included
- [ ] Safari must be the foreground app or recently active for capture to work — document behavior when Safari is backgrounded
- [ ] Content script (`content.js`) triggers capture tick; background script (`background.js`) sends image to native handler

---

## 5. Batch upload and hash chain

- [ ] Batch upload fires at configured batch interval (default 30 s in dev/test builds)
- [ ] Web app shows received batches for this iOS device
- [ ] Web app can decrypt and display Safari screenshots
- [ ] `POST /hash` called per captured screenshot; hash chain visible in web app

---

## 6. App Group shared storage

- [ ] Auth/state files are in the shared App Group container (accessible by both the app and the extension)
- [ ] Runtime overrides written by the app are read by the extension's Rust daemon
- [ ] Safari capture heartbeat / status is written by the extension and read by the app UI
- [ ] App Group entitlement (`group.org.virtueinitiative.virtueios`) present in both app and extension targets

---

## 7. Runtime config override

- [ ] App UI exposes `VIRTUE_BASE_API_URL`, `VIRTUE_CAPTURE_INTERVAL_SECONDS`, `VIRTUE_BATCH_WINDOW_SECONDS` fields
- [ ] Saving overrides applies them to the Rust daemon on next iteration (no restart required)
- [ ] For simulator: use `http://10.7.7.4:8787` to reach the host API (as hardcoded in default overrides)
- [ ] Clearing overrides reverts to defaults

---

## 8. Rust daemon lifecycle (extension process)

- [ ] Daemon runs in the Safari Web Extension process
- [ ] Daemon samples the latest captured frame on each loop iteration
- [ ] Daemon is initialized with `init` called from `SafariWebExtensionHandler.swift`
- [ ] Daemon loop runs as long as the extension is active and Safari is open

---

## 9. Token refresh

- [ ] With a near-expired device access token, the Rust core calls `POST /d/token` automatically
- [ ] Capture and upload continue after token refresh

---

## 10. Sign out

- [ ] Signing out from the app UI:
  - [ ] Sends a logout log
  - [ ] Clears auth / device state from App Group storage
  - [ ] Returns to login UI
- [ ] After sign out, extension daemon idles (no uploads)
- [ ] Re-logging in resumes monitoring

---

## 11. Edge cases

- [ ] Extension disabled mid-session: captures stop; previously queued data continues to upload on next daemon loop
- [ ] No internet: captures queue; uploads retry when connectivity returns
- [ ] API 401: core calls `POST /d/token`, retries once; if refresh fails, daemon idles and logs error
- [ ] API 400: permanent failure logged, not retried
- [ ] Safari not in foreground / Safari closed: verify daemon handles empty / stale latest-frame gracefully; no crash
- [ ] iOS background app refresh disabled: document impact (extension may be suspended; captures may pause)
- [ ] iOS update: verify extension is still enabled and App Group data is preserved after iOS upgrade
- [ ] Multiple Safari tabs open: only the **visible** tab is captured (latest frame from content script)

---

## 12. Simulator vs. device differences

- [ ] Simulator build: basic login, extension enable flow, and capture can be exercised
- [ ] Simulator: Safari extension enablement may behave differently; document any known gaps
- [ ] Physical device: verify App Group entitlements are correctly provisioned (wrong Team ID causes silent storage failures)
- [ ] Physical device: run-on-device script uses correct `--team-id`

---

## 13. Privacy and permission audit

- [ ] Extension requests only the permissions listed in `manifest.json` (no extra host permissions)
- [ ] Extension access is **All Websites** only (user-granted, not hardcoded)
- [ ] No camera, microphone, contacts, or location permissions requested
- [ ] App Group data is not accessible outside the app + extension pair

---

## 14. Version upgrade

- [ ] Installing a newer build over the existing one (via Xcode or TestFlight) completes cleanly
- [ ] Auth state and queued audit data are preserved after upgrade
- [ ] Extension remains enabled after upgrade (iOS may disable extensions on update — document)
