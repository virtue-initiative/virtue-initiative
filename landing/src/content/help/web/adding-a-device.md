---
sidebar_position: 6
---

# Adding a device

You sign a device in from the web app, not by typing your account password into
the device. The device shows a short code, and you enter that code while signed
in here.

## Steps

1. Install Virtue on the device you want to monitor. See the
   [download page](/download) for the installer and the setup steps for that
   platform.
2. Open Virtue on that device and start signing in. It asks for a name for the
   device, then shows a six-character code such as `K7R-M3X`.
3. On this site, open the [Devices page](/app/devices) and select **Add device**.
4. Type the code and select **Continue**. The dash is added for you.
5. Check the device name and platform shown, then select **Add**.

The device finishes signing in within a few seconds and appears on your Devices
page.

## If the code does not work

Codes expire ten minutes after the device shows them. Start the sign-in again on
the device to get a fresh one.

Only enter a code that you are reading off a device in front of you. A code you
were sent by someone else signs their device in to your account, which means
their screenshots land in your logs.

## Signing in with a password instead

Every platform keeps a password option for cases where the code flow will not
work. On Linux, press Enter while the code is showing, or run
`virtue login --password`. On Windows and Android, select **Use a password
instead** on the sign-in screen.
