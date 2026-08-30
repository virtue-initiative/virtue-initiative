## Linux installation

1. If you do not have one, [create an account](/signup).
2. Download the [Linux `.deb` file]({LINUX_DOWNLOAD}).
3. Install the `.deb` file. Once it is installed (and running), you should see an icon in the system tray.
4. run `virtue login` and log in with your email and password.
5. Done! The app will periodically collect screenshots (about once every 5 minutes) and upload them once an hour. You and your partners will be able to view them from the logs page on the website.

## Usage

Run `virtue --help` from a terminal to see the available list of commands.
**Note: after you log out, logging back in will create a seperate device.**

To pause monitoring, run `virtue daemon stop`.
**Note: Virtue will send an alert when monitoring is stopped**

To start Virtue again, run `virtue daemon start`.

## Uninstall

Run `sudo apt remove virtue` to uninstall the package.
