---
name: virtue-login
description: Build, install, and log in to the Virtue Initiative Linux client against the local dev API. Use when the user wants to "log in to virtue", set up / reinstall the virtue client, or register this Linux device with the dev account.
---

# Virtue dev login

Builds the Linux client `.deb` against the local dev API, installs it with `dpkg`, and logs
in. The password prompt is read from a raw terminal, so a pty helper (`login.py`) types it
in; a plain stdin pipe will not work. After the password, `virtue login` also prompts for a
device name, which `login.py` answers too.

The API URL is baked into the binary at compile time. There is no runtime config file, so
pointing the client at a different API means rebuilding it (see `client/README.md`).

## Steps

Run from the repo root. The skill dir is `.claude/skills/virtue-login`.

### 1. Make sure the local API is running

If `./scripts/launch.sh` isn't already running, start it in the background with its output
going to a file, and read the `API :` line from its startup banner. Its port changes on every
run. Don't pipe it into `head` or `tail`, which wedges the API.

### 2. Bake the API URL into the build

Set `VIRTUE_DEFAULT_API_URL` to that URL in the repo-root `.env` (copy `.env.example` if
there is no `.env`). For quick testing, also set `VIRTUE_DEFAULT_CAPTURE_INTERVAL_SECONDS=15`
and `VIRTUE_DEFAULT_BATCH_WINDOW_SECONDS=60`. Say so if you change values that were already
there.

### 3. Build and install the `.deb`

```bash
just linux-install
```

It builds `client/target/debian/virtue-linux_<label>_<arch>.deb` and installs it with
`sudo dpkg -i`. The package's `postinst` (re)starts the per-user `virtue.service`, which
`virtue login` connects to. If it isn't running: `systemctl --user restart virtue.service`.

### 4. Ensure the credentials file exists

`login.py` reads `email`/`password` (and an optional `device_name`) from
`.claude/skills/virtue-login/credentials.json` (gitignored). If it is missing, create it from
the account `just seed` creates:

```json
{
  "email": "dev@dev.com",
  "password": "devpassword"
}
```

See `credentials.example.json` for the format. To point at a different file, set
`VIRTUE_LOGIN_CONFIG=/path/to/creds.json`.

`device_name` is optional:

- omitted/empty — `login.py` presses Enter at the device-name prompt, registering
  the device under the machine hostname (the default).
- set — `login.py` passes it as `virtue login --device-name <name>`, so the device
  registers under that exact name.

### 5. Log in

```bash
python3 .claude/skills/virtue-login/login.py
```

### 6. Confirm

```bash
virtue status --advanced
```

Expect it to show the device signed in, the service running, and the API URL from step 2.

## Notes

- If `status` shows the device signed out after a clean run, check that the API from step 1
  is still running and that the build was made after step 2 (`virtue status --advanced`
  shows the URL it was built with).
- Logging in again when already logged in re-registers a fresh device id — fine
  for dev, but it sends a logout alert if you `virtue logout` first.
- If the build fails, see `client/linux/README.md` and the Linux section of `docs/ci-checks.md`.
