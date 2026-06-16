#!/usr/bin/env python3
"""Drive `virtue login` non-interactively.

The Linux client reads the password from the controlling terminal in raw mode
(crossterm), so it cannot be fed over a normal stdin pipe. This allocates a pty,
runs `virtue login --email <email>`, waits for the "Password:" prompt, types the
password, then answers the "Device name [...]:" prompt that follows.

Credentials are read from a JSON config file (NOT committed):
  { "email": "...", "password": "...", "device_name": "..." }

`device_name` is optional:
  - omitted/empty -> press Enter at the prompt to accept the default (hostname)
  - set          -> pass it via --device-name (no interactive prompt appears)

Config path resolution:
  1. $VIRTUE_LOGIN_CONFIG if set
  2. credentials.json next to this script

See credentials.example.json for the format.
"""

import json
import os
import pty
import select
import sys
import time

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
CONFIG_PATH = os.environ.get(
    "VIRTUE_LOGIN_CONFIG", os.path.join(SCRIPT_DIR, "credentials.json")
)


def load_credentials():
    if not os.path.exists(CONFIG_PATH):
        sys.stderr.write(
            f"Credentials file not found: {CONFIG_PATH}\n"
            "Create it from credentials.example.json with the dev email/password.\n"
        )
        sys.exit(2)
    with open(CONFIG_PATH) as f:
        cfg = json.load(f)
    email = cfg.get("email")
    password = cfg.get("password")
    device_name = (cfg.get("device_name") or "").strip()
    if not email or not password:
        sys.stderr.write(
            f"Config {CONFIG_PATH} must set both 'email' and 'password'.\n"
        )
        sys.exit(2)
    return email, password, device_name


def main() -> int:
    email, password, device_name = load_credentials()

    argv = ["virtue", "login", "--email", email]
    # When a name is provided we pass it as a flag, so no device-name prompt
    # appears. When it is absent we answer the interactive prompt with Enter
    # (accepting the hostname default).
    if device_name:
        argv += ["--device-name", device_name]

    pid, fd = pty.fork()
    if pid == 0:
        # Child: become the virtue client with the pty as its controlling tty.
        os.execvp("virtue", argv)
        os._exit(127)  # unreachable on success

    # Parent: relay output, answer the password and device-name prompts.
    buf = b""
    sent_password = False
    answered_device = device_name != ""  # nothing to answer when flag was passed
    while True:
        try:
            ready, _, _ = select.select([fd], [], [], 60)
        except (OSError, select.error):
            break
        if not ready:
            break
        try:
            data = os.read(fd, 1024)
        except OSError:
            break
        if not data:
            break
        os.write(sys.stdout.fileno(), data)  # mirror prompt/output to our stdout
        buf += data
        if not sent_password and b"Password:" in buf:
            time.sleep(0.2)  # let raw mode engage before sending
            os.write(fd, (password + "\r").encode())
            sent_password = True
        if sent_password and not answered_device and b"Device name [" in buf:
            time.sleep(0.2)
            os.write(fd, b"\r")  # accept the default (hostname)
            answered_device = True

    _, status = os.waitpid(pid, 0)
    return os.waitstatus_to_exitcode(status)


if __name__ == "__main__":
    sys.exit(main())
