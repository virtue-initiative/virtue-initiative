# Manual testing

A short end-to-end checklist to work through by hand on the dev builds before a release (step 3
of "A release, step by step" in [`releasing.md`](releasing.md)). Automated tests are covered in
[`../TESTING.md`](../TESTING.md). The log entries these steps expect come from the tamper checks
in [`../client/core/tampering.md`](../client/core/tampering.md): a late wakeup only counts once
it is more than 2 minutes late, so a kill that is restarted within a minute or so logs nothing.

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
4. Confirm the partner can see logs uploaded after they accepted, and none from before
5. Remove the partnership and confirm they can no longer see the logs

6. Invite an existing user
7. Accept the invite from the other account.
8. Ensure the other user can see the logs

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
3. Sleep and wake the machine; confirm capture resumes automatically and no Screenshot Missed entry appears.
4. Reboot the machine; confirm zero-risk login and logout entries, no Screenshot Missed entry, and that monitoring resumes without re-login.
5. Use Stop Monitoring and Quit from the menu bar; confirm the confirmation prompt, a Monitoring Stopped by User entry and email, and a Monitoring Resumed by User entry with no Screenshot Missed entry when you open Virtue again more than 2 minutes later.
6. Sign out from the window menu; confirm the confirmation prompt, that the device shows Deactivated, and that partners are emailed.
7. Unload the LaunchAgent so the daemon stays down for more than 2 minutes, then load it again; confirm a Screenshot Missed entry.
8. Kill the daemon 20 times within 10 minutes; confirm a Repeated Restarts entry, emailed to partners straight away.
9. Revoke Screen Recording in System Settings; confirm capture stops and a Capture Failed entry appears after five failed screenshots; restoring permission resumes capture.

## Windows

Install the MSIX and launch from Start.

1. Confirm the tray icon appears and no window opens automatically.
2. Sign in from the window and confirm screenshots appear in the web app within ~10 minutes.
3. Sleep and wake the machine; confirm capture resumes automatically and no Screenshot Missed entry appears.
4. Reboot the machine; confirm zero-risk login and logout entries, no Screenshot Missed entry, and that monitoring restarts via the startup task without re-login.
5. Sign out of the Windows user session and back in; confirm a zero-risk logout entry and no Screenshot Missed entry.
6. Use tray → Exit; confirm the confirmation prompt, a Monitoring Stopped by User entry and email, and a Monitoring Resumed by User entry with no Screenshot Missed entry when you start Virtue again.
7. Select **Stop monitoring** in the window; confirm the same prompt and entries as tray → Exit.
8. Open the window menu (**☰**) and choose **Sign out**; confirm the confirmation prompt, that the device shows Deactivated, and that partners are emailed.
9. Force-kill `Virtue.WindowsApp.exe` from Task Manager (End Task); confirm the watchdog relaunches it within a minute and no entry is logged.
10. Disable the `VirtueTrayStartup` task and reboot, then start Virtue by hand more than 2 minutes after signing in; confirm a Screenshot Missed entry.

## Linux

Install the `.deb` and verify.

1. Run `virtue login` and confirm `virtue status` shows "Monitoring active".
2. Confirm screenshots appear in the web app within ~10 minutes.
3. Suspend and resume the machine; confirm capture resumes automatically and no Screenshot Missed entry appears.
4. Reboot the machine; confirm zero-risk login and logout entries, no Screenshot Missed entry, and that the systemd user service starts automatically without re-login.
5. Run `virtue daemon stop`; confirm the confirmation prompt and a Monitoring Stopped by User entry and email. Wait more than 2 minutes, run `virtue daemon start`, and confirm a Monitoring Resumed by User entry with no Screenshot Missed entry.
6. Run `virtue logout`; confirm the confirmation prompt, that the device shows Deactivated, and that partners are emailed.
7. Run `systemctl --user stop virtue.service` (outside `virtue daemon stop`), wait more than 2 minutes and start it again; confirm a Screenshot Missed entry.
8. `kill -9 <daemon-pid>` once; confirm systemd restarts it within 5 seconds and no entry is logged.
9. Kill the daemon 20 times within 10 minutes; confirm a Repeated Restarts entry, emailed to partners straight away.
10. On Wayland without a supported capture tool, confirm the client logs a capture failure but does not crash and continues retrying uploads.

## Android

Install the APK and launch.

1. Turn on the Virtue Screen Monitor Accessibility service from the setup guide, then log in; confirm a persistent notification appears.
2. Confirm screenshots appear in the web app within ~10 minutes.
3. Swipe the app away from Recents; confirm monitoring continues via the foreground service.
4. Reboot the device; confirm monitoring restarts automatically and new activity appears in the web app.
5. Force-stop the app from Android Settings → Apps and leave it more than 2 minutes; confirm a Screenshot Missed entry once monitoring resumes.
6. Pause monitoring; confirm the confirmation prompt, a Monitoring Stopped by User entry and email, and a Monitoring Resumed by User entry after resuming.
7. Turn off the Accessibility service mid-session; confirm the app does not crash, capture stops, and capture resumes when it is turned back on.
8. Sign out; confirm the device shows Deactivated, partners are emailed, and the login screen is shown.

## iOS

Build and run on a device or simulator.

1. Log in and enable the Safari extension in iOS Settings with All Websites access.
2. Browse in Safari and confirm screenshots appear in the web app within ~5 minutes.
3. Disable the Safari extension; confirm Check Extension reports it off, new captures stop, any queued data still uploads, and no Screenshot Missed entry appears (iOS has no tamper detection).
4. Force-quit Safari or the extension and reopen; confirm capture resumes when Safari is active again.
5. Pause monitoring; confirm the confirmation prompt and a Monitoring Stopped by User entry and email.
6. Sign out; confirm the device shows Deactivated, partners are emailed, and the login screen is shown.

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
11. Ensure it says an "email was sent if an account exists" but does not send an email
