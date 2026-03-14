---
sidebar_position: 3
---

# Linux installation

If you do not yet have one, first [create an account](https://app.virtueinitiative.org/#signup).

Download the Linux `.deb` file from the [downloads](/download) page.

Double-click the downloaded file to install the package.

When the `virtue` service is installed and running, you should see an icon appear in the system tray.

From a terminal, run `virtue login` and login with your credentials.

That's it! At this point the app will begin to periodically collect and send logs and images in the background.

# Usage

Run `virtue --help` from a terminal to see the available list of commands.
**Note that the service will log an alert for logout events.**

To stop the service, run `systemctl --user stop virtue.service`.
**Note that the service will log an alert when stopping the service.**

To start the service again, run `systemctl --user start virtue.service`.

# Uninstall

Run `sudo apt remove virtue` to uninstall the package. **Note that the service
will log an alert as it shuts down.**
