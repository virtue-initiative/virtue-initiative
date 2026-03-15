# Testing

Use this as a short manual end-to-end checklist before shipping.

## Signup Flow

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

## Platform checks

1. Android: app survives backgrounding and reboot.
2. iOS: Safari extension captures after app relaunch.
3. Linux: service restarts cleanly with `systemctl --user restart virtue.service`.
4. macOS: capture works after granting Screen Recording.
5. Windows: service restart resumes capture.

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
