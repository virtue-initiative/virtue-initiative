# QA Checklist

Manual end-to-end checks before a release. Fill in the header before each run.

**App version:**
**API env (prod / staging / local):**
**Tester:**
**Date:**

---

## Web

- [ ] Signing up with a new email sends a verification email; clicking the link creates the account and lands on the dashboard
- [ ] Signing up with an already-registered email shows an appropriate error
- [ ] Logging in with valid credentials succeeds; invalid credentials show an error
- [ ] Account settings page loads and shows the current user's email
- [ ] Sending a partner invitation by email delivers an invite email to the recipient
- [ ] Recipient follows the invite link, signs up (or logs in), and appears as a partner on the inviter's dashboard
- [ ] Partner can view the inviter's device activity and screenshots
- [ ] Removing a partner from the settings page revokes their access immediately
- [ ] Email digest preference can be saved and survives a page reload

---

## macOS

Install the `.dmg`, drag to Applications, and launch.

- [ ] Tray icon appears in the menu bar after launch
- [ ] Opening the tray menu and logging in with valid credentials succeeds; invalid credentials show an error
- [ ] After login, screenshots appear in the web app logs within ~10 minutes
- [ ] Stopping monitoring via the tray menu prompts for confirmation, then removes the tray icon and stops uploads
- [ ] Reopening the app restarts monitoring without requiring re-login
- [ ] Logging out via the tray menu prompts for confirmation, clears the session, and shows the login dialog on next open
- [ ] After a reboot, monitoring resumes automatically and a new activity entry appears in the web app
- [ ] Revoking Screen Recording permission in System Settings causes capture to stop; restoring it resumes capture within the next interval
- [ ] With no internet connection, the client queues events and uploads them once connectivity is restored

---

## Windows

Install the MSIX and launch from Start.

- [ ] Tray icon appears after install; no window opens automatically
- [ ] Opening the settings window and logging in with valid credentials succeeds; invalid credentials show an error
- [ ] After login, screenshots appear in the web app logs within ~10 minutes
- [ ] Closing the settings window hides it back to the tray without exiting the app
- [ ] Tray → Exit prompts for confirmation, stops monitoring, and exits without logging out
- [ ] Relaunching from Start resumes monitoring without re-login
- [ ] After a reboot, monitoring starts automatically via the startup task and new activity appears in the web app
- [ ] Logging out from the settings window clears the session; next launch shows the login screen
- [ ] With no internet connection, the client queues events and uploads them once connectivity is restored

---

## Linux

Install the `.deb` and verify.

- [ ] `virtue login` prompts for credentials; valid credentials succeed, invalid credentials show an error
- [ ] `virtue status` shows logged in and monitoring active after login
- [ ] After login, screenshots appear in the web app logs within ~10 minutes
- [ ] `virtue daemon stop` stops monitoring and records a user-stop event visible in the web app
- [ ] `virtue daemon start` resumes monitoring; new activity appears in the web app
- [ ] `virtue logout` clears the session; `virtue status` shows logged out; monitoring stops
- [ ] After a reboot, the systemd service starts automatically and new activity appears in the web app
- [ ] On Wayland without a supported capture tool, the client logs a capture failure but does not crash and retries uploads for any previously queued data
- [ ] With no internet connection, the client queues events and uploads them once connectivity is restored

---

## Android

Install the APK (debug or release) and launch.

- [ ] Login screen accepts credentials; valid credentials succeed, invalid credentials show an error
- [ ] After login, the app prompts for screen capture permission (MediaProjection); granting it starts the foreground monitoring service with a persistent notification
- [ ] After permission is granted, screenshots appear in the web app logs within ~10 minutes
- [ ] Closing the app from Recents does not stop monitoring; the persistent notification remains and uploads continue
- [ ] After a device reboot, monitoring restarts automatically and new activity appears in the web app
- [ ] Signing out sends a stop event visible in the web app, stops the service, and returns to the login screen
- [ ] With no internet connection, the client queues events and uploads them once connectivity is restored

---

## iOS

Build and run on a device or simulator.

- [ ] Login screen accepts credentials; valid credentials succeed, invalid credentials show an error
- [ ] After login, the app prompts the user to enable the Safari extension in iOS Settings; enabling it with All Websites access allows capture to begin
- [ ] Browsing in Safari causes screenshots to appear in the web app logs within ~5 minutes
- [ ] Disabling the Safari extension stops new captures; previously queued data still uploads
- [ ] Signing out clears the session and stops uploads; the login screen is shown on next open
- [ ] With no internet connection, the client queues events and uploads them once connectivity is restored
