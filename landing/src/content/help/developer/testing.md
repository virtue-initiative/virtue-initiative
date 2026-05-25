# Testing

Use this as a short manual end-to-end checklist before shipping.

## Signup flow

1. Create a fresh account.
2. Log in on one client device.
3. Confirm the device appears in the web dashboard.
4. Wait for screenshots to upload.
5. Confirm logs and screenshots render on web.
6. Confirm gallery view works

## Partners

1. Invite a non-existent account from web.
2. Ensure an email is sent and can be used to sign up with an account
3. Ensure the account is automatically added as a partner
4. Confirm that there is clear indication of why the partner cannot see logs yet
5. Click confirm partner on the original account
6. Confirm the partner can see the logs
7. Remove the partnership and confirm they can no longer see the logs

8. Invite an existing user
9. Accept the invite from the other account.
10. Ensure the other user can accept it

## Session and recovery

1. Log out and log back in on web.
2. Restart the client machine.
3. Confirm monitoring resumes automatically.

4. Confirm a bad email/password is rejected in the client
5. Confirm a bad email/password is rejected on the web

## Capture checks

1. Verify the capture cadence matches the configured interval.
2. Leave the client idle long enough to force at least one batch upload.
3. Confirm timestamps are ordered and recent.
4. Confirm login/logout events

## macOS

Install the `.dmg`, drag to Applications, and launch.

1. Confirm the tray icon appears in the menu bar.
2. Log in from the tray menu and confirm screenshots appear in the web app within ~10 minutes.
3. Sleep and wake the machine; confirm zero-risk suspend and wake logs and that capture resumes automatically.
4. Reboot the machine; confirm zero-risk shutdown and startup logs and that monitoring resumes without re-login.
5. Use tray → Stop Monitoring; confirm the confirmation prompt and a high-risk user-stop log with a marker so the next start is zero-risk.
6. Use tray → Logout; confirm the confirmation prompt and a high-risk logout log.
7. `kill <daemon-pid>` outside a system shutdown; confirm a 0.5-risk stop log followed by a startup classification based on the stop marker (zero-risk if within 10 seconds, otherwise high-risk).
8. `kill -9 <daemon-pid>`; confirm no stop log is emitted and the next startup emits a high-risk log if the gap exceeds the 70-second ping window.
9. Revoke Screen Recording in System Settings; confirm capture stops and a high-risk log is emitted for the permission loss; restoring permission resumes capture.

## Windows

Install the MSIX and launch from Start.

1. Confirm the tray icon appears and no window opens automatically.
2. Log in from the settings window and confirm screenshots appear in the web app within ~10 minutes.
3. Sleep and wake the machine; confirm zero-risk suspend and wake logs and that capture resumes automatically.
4. Reboot the machine; confirm zero-risk shutdown and startup logs and that monitoring restarts via the startup task without re-login.
5. Sign out of the Windows user session; confirm a zero-risk session-logout log.
6. Use tray → Exit; confirm the confirmation prompt and a high-risk user-stop log with a marker so the next start is zero-risk.
7. Log out from the settings window; confirm a high-risk logout log.
8. Force-kill `Virtue.WindowsApp.exe` from Task Manager (End Task); confirm a 0.5-risk stop log if cleanup runs, or a high-risk log on next start determined by ping timestamps.
9. Disable the `VirtueTrayStartup` task and reboot; confirm a high-risk log on the next time the app runs.

## Linux

Install the `.deb` and verify.

1. Run `virtue login` and confirm `virtue status` shows logged in and monitoring active.
2. Confirm screenshots appear in the web app within ~10 minutes.
3. Suspend and resume the machine; confirm zero-risk suspend and wake logs and that capture resumes automatically.
4. Reboot the machine; confirm zero-risk shutdown and startup logs and that the systemd user service starts automatically without re-login.
5. Run `virtue daemon stop`; confirm the confirmation prompt and a high-risk user-stop log with a marker so the next `virtue daemon start` is zero-risk.
6. Run `virtue logout`; confirm the confirmation prompt and a high-risk logout log.
7. Run `systemctl --user stop virtue.service` (outside `virtue daemon stop`); confirm a 0.5-risk stop log and a startup classification based on the stop marker on next start.
8. `kill <daemon-pid>` (SIGTERM) outside a system shutdown; confirm a 0.5-risk stop log.
9. `kill -9 <daemon-pid>` (SIGKILL); confirm no stop log is emitted and the next startup emits a high-risk log if the gap exceeds the 70-second ping window.
10. On Wayland without a supported capture tool, confirm the client logs a capture failure but does not crash and continues retrying uploads.

## Android

Install the APK and launch.

1. Log in and grant the screen capture (MediaProjection) permission; confirm a persistent foreground notification appears.
2. Confirm screenshots appear in the web app within ~10 minutes.
3. Swipe the app away from Recents; confirm monitoring continues via the foreground service.
4. Reboot the device; confirm monitoring restarts automatically and new activity appears in the web app.
5. Force-stop the app from Android Settings → Apps; confirm a high-risk tamper log is emitted and monitoring resumes on next launch.
6. Revoke screen-capture permission mid-session; confirm a high-risk log is emitted for the permission loss, the app does not crash, and capture resumes when permission is re-granted.
7. Sign out; confirm a high-risk logout log, the service stops, and the login screen is shown.

## iOS

Build and run on a device or simulator.

1. Log in and enable the Safari extension in iOS Settings with All Websites access.
2. Browse in Safari and confirm screenshots appear in the web app within ~5 minutes.
3. Disable the Safari extension; confirm a high-risk tamper log is emitted, new captures stop, and any queued data still uploads.
4. Force-quit Safari or the extension and reopen; confirm capture resumes when Safari is active again.
5. Sign out; confirm a high-risk logout log and the login screen is shown.

## Specific tests

These tests might not need to be run every time.

### Email change

1. Go to settings
2. Change your email
3. Ensure new email is set to unverified
4. Ensure validation email is sent and works

### Password reset

1. Go to login
2. Click reset password
3. Enter email
4. Follow password reset instructions
5. Ensure password is reset and can be used to log in
6. Ensure partners can see new logs
7. Ensure you can see other partners' logs

8. Go to login
9. Click reset password
10. Enter an email without an account
11. Ensure it says an "email was sent if an account exists" but doesn't send an email
