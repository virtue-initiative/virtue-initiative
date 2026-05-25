# QA Checklist

Manual end-to-end checks before a release. Fill in the header before each run.

**App version:**
**API env (prod / staging / local):**
**Tester:**
**Date:**

## Notes on tamper alerts

Tamper detection is meant to distinguish clean stops (user-confirmed stop, logout, system shutdown, sleep) from suspicious stops (force-kill, service disable, missing process after reboot). Clean stops should appear as low-risk activity in the web app. Suspicious stops should appear as a high-risk alert in the web app, usually surfaced on the next time the daemon comes back up. Suspend/wake and screen-recording permission changes are informational and should not surface as high-risk.

Tamper coverage by platform:

- macOS, Windows, Linux: service-stop tamper alerts are intended to work end-to-end
- Android, iOS: lifecycle/service-stop tamper alerts are not currently tracked; only capture-permission state is observable

---

## Web

- [ ] Signing up with a new email sends a verification email; clicking the link creates the account and lands on the dashboard
- [ ] Signing up with an already-registered email shows an appropriate error
- [ ] Logging in with valid credentials succeeds; invalid credentials show an error
- [ ] Account settings page loads and shows the current user's email
- [ ] Sending a partner invitation by email delivers an invite email to the recipient
- [ ] Recipient follows the invite link, signs up (or logs in), and appears as a partner on the inviter's dashboard
- [ ] Partner can view the inviter's device activity and screenshots
- [ ] High-risk tamper alerts from any device show up prominently on the dashboard and in the partner's view
- [ ] Removing a partner from the settings page revokes their access immediately
- [ ] Email digest preference can be saved and survives a page reload

---

## macOS

Install the `.dmg`, drag to Applications, and launch.

- [ ] Tray icon appears in the menu bar after launch
- [ ] Opening the tray menu and logging in with valid credentials succeeds; invalid credentials show an error
- [ ] After login, screenshots appear in the web app logs within ~10 minutes
- [ ] Stopping monitoring via the tray menu prompts for confirmation, then removes the tray icon and stops uploads — recorded as a low-risk user-stop event
- [ ] Reopening the app restarts monitoring without requiring re-login
- [ ] Logging out via the tray menu prompts for confirmation, clears the session, and shows the login dialog on next open — recorded as a low-risk event
- [ ] Normal shutdown / restart records a low-risk shutdown event; monitoring resumes after reboot and a new activity entry appears
- [ ] Sleep / wake records low-risk suspend and wake events; monitoring resumes without intervention
- [ ] `kill <daemon-pid>` (SIGTERM) outside of a system shutdown produces a high-risk tamper alert in the web app once the daemon is back up
- [ ] `kill -9 <daemon-pid>` (SIGKILL) produces a high-risk tamper alert in the web app once the daemon is back up
- [ ] Unloading the LaunchAgent (`launchctl bootout …`) outside the tray's Stop Monitoring flow produces a high-risk tamper alert
- [ ] Revoking Screen Recording permission in System Settings is recorded as an informational permission-change event (not high-risk) and capture stops; restoring permission resumes capture within the next interval
- [ ] With no internet connection, the client queues events and uploads them once connectivity is restored

---

## Windows

Install the MSIX and launch from Start.

- [ ] Tray icon appears after install; no window opens automatically
- [ ] Opening the settings window and logging in with valid credentials succeeds; invalid credentials show an error
- [ ] After login, screenshots appear in the web app logs within ~10 minutes
- [ ] Closing the settings window hides it back to the tray without exiting the app
- [ ] Tray → Exit prompts for confirmation, stops monitoring, and exits without logging out — recorded as a low-risk user-stop event
- [ ] Relaunching from Start resumes monitoring without re-login
- [ ] Normal shutdown / restart records a low-risk shutdown event; monitoring starts automatically after reboot and new activity appears
- [ ] Sleep / wake records low-risk suspend and wake events; monitoring resumes without intervention
- [ ] Signing out of the Windows user session is recorded as a low-risk session-logout event
- [ ] Force-killing `Virtue.WindowsApp.exe` from Task Manager (End Task) outside of Tray → Exit produces a high-risk tamper alert in the web app once the app is back up
- [ ] Disabling the `VirtueTrayStartup` task and rebooting produces a high-risk tamper alert on the next time the app runs (missed-process / startup-recovery)
- [ ] Logging out from the settings window clears the session; next launch shows the login screen
- [ ] With no internet connection, the client queues events and uploads them once connectivity is restored

---

## Linux

Install the `.deb` and verify.

- [ ] `virtue login` prompts for credentials; valid credentials succeed, invalid credentials show an error
- [ ] `virtue status` shows logged in and monitoring active after login
- [ ] After login, screenshots appear in the web app logs within ~10 minutes
- [ ] `virtue daemon stop` stops monitoring — recorded as a low-risk user-stop event in the web app
- [ ] `virtue daemon start` resumes monitoring; new activity appears in the web app
- [ ] `virtue logout` clears the session; `virtue status` shows logged out; monitoring stops — recorded as a low-risk event
- [ ] Normal shutdown / restart records a low-risk shutdown event; the systemd service starts automatically after reboot and new activity appears
- [ ] Sleep / wake records low-risk suspend and wake events; monitoring resumes without intervention
- [ ] `kill <daemon-pid>` (SIGTERM) outside of a system shutdown produces a high-risk tamper alert in the web app once the daemon is back up
- [ ] `kill -9 <daemon-pid>` (SIGKILL) produces a high-risk tamper alert in the web app once the daemon is back up
- [ ] `systemctl --user stop virtue.service` outside of `virtue daemon stop` produces a high-risk tamper alert (service-manager stop without recorded user intent)
- [ ] Disabling the unit (`systemctl --user disable virtue.service`) and rebooting produces a high-risk tamper alert on the next daemon run
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
- [ ] Force-stopping the app from Android Settings → Apps stops uploads; monitoring restarts on next launch (no tamper alert is expected — Android service-stop tamper is not currently tracked)
- [ ] Revoking screen-capture permission stops new captures; the app does not crash and resumes when permission is re-granted
- [ ] Signing out sends a stop event visible in the web app, stops the service, and returns to the login screen
- [ ] With no internet connection, the client queues events and uploads them once connectivity is restored

---

## iOS

Build and run on a device or simulator.

- [ ] Login screen accepts credentials; valid credentials succeed, invalid credentials show an error
- [ ] After login, the app prompts the user to enable the Safari extension in iOS Settings; enabling it with All Websites access allows capture to begin
- [ ] Browsing in Safari causes screenshots to appear in the web app logs within ~5 minutes
- [ ] Disabling the Safari extension stops new captures; previously queued data still uploads (no tamper alert is expected — iOS service-stop tamper is not currently tracked)
- [ ] Force-quitting the app or the Safari extension stops new captures and resumes when the user opens Safari again
- [ ] Signing out clears the session and stops uploads; the login screen is shown on next open
- [ ] With no internet connection, the client queues events and uploads them once connectivity is restored
