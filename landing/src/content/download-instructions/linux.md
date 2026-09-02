## Linux installation

1. If you do not have one, [create an account](/signup).
2. Download the [Linux `.deb` file]({LINUX_DOWNLOAD}).
3. Install the `.deb` file. Once it is installed (and running), you should see an icon in the system tray.
4. Run `virtue login`. It asks for a device name, then shows a six-character code.
5. Enter that code on the [Devices page](/app/devices?add) of the web app, under **Add device**. See [Adding a device](/help/web/adding-a-device) for the full walkthrough.
6. Done! The app will periodically collect screenshots (about once every 5 minutes) and upload them once an hour. You and your partners will be able to view them from the logs page on the website.

## Usage

Run `virtue --help` from a terminal to see the available list of commands.

**Note: after you log out, logging back in will create a seperate device.**

To sign in with your email and password instead of a code, press Enter while the code is showing, or run `virtue login --password`.

To pause monitoring, run `virtue daemon stop`.
**Note: Virtue will send an alert when monitoring is stopped**

To start Virtue again, run `virtue daemon start`.

## Uninstall

Run `sudo apt remove virtue` to uninstall the package.
