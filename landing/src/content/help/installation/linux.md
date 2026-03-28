---
sidebar_position: 3
---

# Linux client

## Installation

If you do not yet have one, first [create an account](/signup).

Download the Linux `.deb` file from the [downloads](/download) page.

Double-click the downloaded file to install the package.

When the `virtue` service is installed and running, you should see an icon appear in the system tray.

From a terminal, run `virtue login` and log in with your credentials.

That's it! At this point the app will begin to periodically collect and send events and screenshots in the background. To ensure that the service is functioning properly, log in to the [application](/app) and you should see a new device in your dashboard. Click on the Logs tab and for your device you should start seeing screenshots. A few screenshots should appear within the first 5-10 minutes, and then batches of images should appear every hour.

## Usage

Run `virtue --help` from a terminal to see the available list of commands.
**Note that the service will log an alert for logout events.**

To stop the service, run `systemctl --user stop virtue.service`.
**Note that the service will log an alert when stopping the service.**

To start the service again, run `systemctl --user start virtue.service`.

## Uninstall

Run `sudo apt remove virtue` to uninstall the package. **Note that the service
will log an alert as it shuts down.**
