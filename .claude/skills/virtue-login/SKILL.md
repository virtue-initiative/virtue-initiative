---
name: virtue-login
description: Build, install, and log in to the Virtue Initiative Linux client against the local dev API. Use when the user wants to "log in to virtue", set up / reinstall the virtue client, or register this Linux device with the dev account.
---

# Virtue dev login

Builds the Linux client `.deb`, installs it with `dpkg`, points it at the local
dev API, and logs in with the dev account. The password prompt is read from a raw
terminal, so a pty helper (`login.py`) types it in — a plain stdin pipe will not work.
After the password, `virtue login` also prompts for a device name; `login.py`
answers it automatically (see step 5/6).

Credentials live in `credentials.json` (gitignored), NOT in the script. The dev
account is:

- email: `test1@virtueinitiative.org`
- password: `test1@virtueinitiative.org`

Local dev API: `http://localhost:8787` (this profile's wrangler dev server).

## Steps

Run from the repo root. The skill dir is `.claude/skills/virtue-login`.

### 1. Build the `.deb`

```bash
( cd client && ./linux/scripts/build-deb.sh )
```

Output lands in `client/target/debian/virtue-linux_<label>_<arch>.deb`.

### 2. Install it

```bash
sudo dpkg -i client/target/debian/virtue-linux_*.deb
```

The package `postinst` enables and (re)starts the per-user `virtue.service`.

### 3. Point the client at the local dev API

Only write this if it is missing or pointing elsewhere — don't clobber an existing
custom config without saying so.

```bash
mkdir -p ~/.config/virtue
cat > ~/.config/virtue/config.json <<'EOF'
{
  "api_base_url": "http://localhost:8787",
  "capture_interval_seconds": 15,
  "batch_window_seconds": 60
}
EOF
```

### 4. Make sure the daemon is up

`virtue login` connects to the daemon socket, so the service must be running:

```bash
systemctl --user restart virtue.service
```

### 5. Ensure the credentials file exists

`login.py` reads `email`/`password` (and an optional `device_name`) from
`.claude/skills/virtue-login/credentials.json` (gitignored). On first use, if it
is missing, create it from the dev account:

```bash
cat > .claude/skills/virtue-login/credentials.json <<'EOF'
{
  "email": "test1@virtueinitiative.org",
  "password": "test1@virtueinitiative.org"
}
EOF
```

See `credentials.example.json` for the format. To point at a different file, set
`VIRTUE_LOGIN_CONFIG=/path/to/creds.json`.

`device_name` is optional:

- omitted/empty — `login.py` presses Enter at the device-name prompt, registering
  the device under the machine hostname (the default).
- set — `login.py` passes it as `virtue login --device-name <name>`, so the device
  registers under that exact name.

### 6. Log in

```bash
python3 .claude/skills/virtue-login/login.py
```

### 7. Confirm

```bash
virtue status
```

Expect `logged_in: true`, `running: true`, and `base_api_url: http://localhost:8787`.

## Notes

- The local dev API must be running (wrangler on `:8787`). If `status` shows
  `logged_in: false` after a clean run, check that the dev server is up
  (`./scripts/dev-setup.sh`) and that port 8787 is listening.
- Logging in again when already logged in re-registers a fresh device id — fine
  for dev, but it sends a logout alert if you `virtue logout` first.
- If the build fails, run the area's checks per `AGENTS.md` / `client/AGENTS.md`.
