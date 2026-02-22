# BePure macOS Client

This client has two modes in one binary:

- Tray app (default): menu bar icon + login/logout dialogs
- Daemon (`daemon`): background capture + upload worker

## Behavior

- On tray app launch, it installs/starts a LaunchAgent (`codes.anb.bepure.daemon`) so the daemon restarts on login/reboot.
- Daemon idles until login is complete (token + device ID).
- Menu action `Open BePure`:
  - If logged out: prompts for email/password.
  - If logged in: shows signed-in state with a `Logout` action.

## Screen capture permission

macOS may block screenshot capture until Screen Recording permission is granted for the app/binary.
If captures fail, grant permission under:

`System Settings -> Privacy & Security -> Screen Recording`

## Build

From `client/`:

```bash
cargo build --release -p bepure-mac-client
```

## Build `.app`

```bash
./mac/scripts/build-app.sh
```

Creates:

`client/target/macos/BePure.app`

## Build `.dmg` (drag to Applications)

```bash
./mac/scripts/build-dmg.sh
```

Creates:

`client/target/macos/BePure-<version>.dmg`
