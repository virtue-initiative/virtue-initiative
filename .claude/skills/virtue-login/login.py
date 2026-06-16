#!/usr/bin/env python3
"""Drive `virtue login` non-interactively.

The Linux client reads the password from the controlling terminal in raw mode
(crossterm), so it cannot be fed over a normal stdin pipe. This allocates a pty,
runs `virtue login --email <email>`, waits for the "Password:" prompt, and types
the password followed by Enter.

Credentials are read from a JSON config file (NOT committed):
  { "email": "...", "password": "..." }

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
    if not email or not password:
        sys.stderr.write(
            f"Config {CONFIG_PATH} must set both 'email' and 'password'.\n"
        )
        sys.exit(2)
    return email, password


def main() -> int:
    email, password = load_credentials()

    pid, fd = pty.fork()
    if pid == 0:
        # Child: become the virtue client with the pty as its controlling tty.
        os.execvp("virtue", ["virtue", "login", "--email", email])
        os._exit(127)  # unreachable on success

    # Parent: relay output, send the password when prompted.
    buf = b""
    sent = False
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
        if not sent and b"Password:" in buf:
            time.sleep(0.2)  # let raw mode engage before sending
            os.write(fd, (password + "\r").encode())
            sent = True

    _, status = os.waitpid(pid, 0)
    return os.waitstatus_to_exitcode(status)


if __name__ == "__main__":
    sys.exit(main())
