# Accessing the virtue-win11 VM

The `virtue-win11` VM runs Windows 11 on the Linux host via libvirt/KVM. SSH is pre-configured.
All `virsh` commands require `--connect qemu:///system`.

## Start / stop the VM

```bash
virsh --connect qemu:///system start virtue-win11
virsh --connect qemu:///system shutdown virtue-win11   # graceful
virsh --connect qemu:///system destroy virtue-win11    # force off
virsh --connect qemu:///system list --all              # check state
```

## Wait for boot / SSH poll

After starting the VM, poll until SSH is ready rather than sleeping a fixed amount:

```bash
until ssh -o ConnectTimeout=5 -o BatchMode=yes virtue-win11 "echo ready" 2>/dev/null; do sleep 5; done
echo "SSH is up"
```

## Take a screenshot

`virsh screenshot` captures the display without needing a graphical session:

```bash
virsh --connect qemu:///system screenshot virtue-win11 /tmp/vm-screen.ppm
convert /tmp/vm-screen.ppm -resize 50% /tmp/vm-screen.png
# Then read /tmp/vm-screen.png with the Read tool (it supports images)
```

## Unlock the lock screen

The lock screen has two states:

1. **Clock view** — shows the time and "Enter your PIN" at the top. Press Enter to reveal the full PIN prompt.
2. **PIN prompt** — shows the user avatar and a PIN input field. Type the PIN and press Enter.

```bash
# Wake the screen and show the PIN input field:
virsh --connect qemu:///system send-key virtue-win11 --codeset linux KEY_ENTER

# Take a screenshot to confirm the PIN input field is showing, then enter the PIN (1212):
virsh --connect qemu:///system send-key virtue-win11 --codeset linux KEY_1 KEY_2 KEY_1 KEY_2 KEY_ENTER
```

After entering the PIN, wait a few seconds and take a screenshot to confirm the desktop appears.

## SSH access

The VM has a single account: **Andrew Baumes**, signed in via Microsoft account
`help@virtueinitiative.org` (local username `help`, profile `C:\Users\help`), which is also
the account SSH connects as. SSH is configured in `~/.ssh/config` as `Host virtue-win11`
(help@192.168.122.128). The VM must be running and fully booted before SSH is available (use
the poll loop above).

```bash
ssh virtue-win11 "powershell <command>"
```

SSH sessions get a fully elevated token automatically (UAC's split-token filtering only
applies to interactive logons, not network/SSH logons), so no separate Administrator account
or UAC workaround is needed to run elevated commands over SSH.

**Note:** SSH still runs in a separate Windows session from the interactive desktop — GUI
launch via `Start-Process shell:AppsFolder\...` will **not** show a window on the desktop, and
`Get-Process` won't see GUI apps running in the interactive session. Use the virsh keyboard
method below to launch GUI apps and see them on screen.

## Reach the host's dev stack from the VM

`scripts/launch.sh <domain>` on the host serves `app.<domain>.localhost` and
friends through Caddy. Windows resolves every name ending in `.localhost` to
loopback inside `getaddrinfo`, ahead of the hosts file, so hosts entries do not
work here: `ping` follows them but curl, browsers and .NET do not. Forwarding
the VM's own loopback ports to the host does work, and Caddy routes on the Host
header, so ports 80 and 443 cover every dev domain at once.

```bash
just windows-vm-network        # once per VM rebuild
```

That sets up the port proxy and trusts Caddy's local CA (needed only for
`https://` in a browser inside the VM; the Rust client can use the `http://`
URL and needs no trust store changes). Afterwards, from inside the VM:

```
http://app.<domain>.localhost          web app
http://app.<domain>.localhost/api      API
http://<domain>.localhost              landing
```

The client's API URL is compile-time, so point a build at it with:

```bash
just windows-build-ssh --mode msix --api-url http://app.<domain>.localhost/api
```

## Build the MSIX

Run from the Linux host repo root. Syncs local source to the VM and builds:

```bash
bash client/windows/scripts/remote-windows-build.sh \
  --build-host virtue-win11 \
  --mode msix \
  --profile Debug
```

The MSIX and setup bundle are left on the VM at:

- `C:\virtue-build\src\client\windows\dist\virtue-windows-0.0.7-dev.msix`
- `C:\virtue-build\src\client\windows\dist\virtue-windows-0.0.7-dev-setup\` (directory with installer)

To rebuild without re-uploading source (faster when only the VM-side changed):

```bash
bash client/windows/scripts/remote-windows-build.sh \
  --build-host virtue-win11 \
  --mode msix \
  --profile Debug \
  --skip-sync
```

## Install the MSIX

Run the generated install script on the VM (handles cert trust and package installation):

```bash
ssh virtue-win11 "powershell -NoProfile -ExecutionPolicy Bypass -File \
  'C:\virtue-build\src\client\windows\dist\virtue-windows-0.0.7-dev-setup\Install.ps1'"
```

## Launch the app

Because SSH runs in a different session from the interactive desktop, use virsh keyboard input
to search and launch from the Start menu:

```bash
# Open Start menu
virsh --connect qemu:///system send-key virtue-win11 --codeset linux KEY_LEFTMETA

# Type "virtue" and press Enter to launch
virsh --connect qemu:///system send-key virtue-win11 --codeset linux \
  KEY_V KEY_I KEY_R KEY_T KEY_U KEY_E KEY_ENTER

# Wait ~2s and take a screenshot to confirm the app window appeared
```

The app runs in the tray and shows a sign-in form on first launch. The package family name is
`VirtueInitiative.VirtueWindows_akvj6hh7d4m2e`.

## Read the client log

The Virtue Windows client writes daily-rotated logs to
`C:\ProgramData\Virtue\data\logs\virtue.<date>.log`.

```bash
# Last 50 lines (adjust the date suffix to today's log file)
ssh virtue-win11 "powershell Get-ChildItem 'C:\ProgramData\Virtue\data\logs' | Sort-Object LastWriteTime -Descending | Select-Object -First 1 | Get-Content -Tail 50"

# Live tail of the newest log file
ssh virtue-win11 "powershell Get-Content -Wait -Tail 20 (Get-ChildItem 'C:\ProgramData\Virtue\data\logs' | Sort-Object LastWriteTime -Descending | Select-Object -First 1).FullName"
```

## Check if the client is running

```bash
ssh virtue-win11 "powershell Get-Process -Name '*virtue*' -ErrorAction SilentlyContinue"
ssh virtue-win11 "powershell Get-Service -Name '*Virtue*' -ErrorAction SilentlyContinue"
ssh virtue-win11 "powershell Get-AppxPackage -Name 'VirtueInitiative.VirtueWindows'"
```

Note: `Get-Process` may return nothing even when the app is running, because the app runs in the
interactive desktop session while SSH runs in a separate (non-interactive) session, even
though both are the same user account.

## Send keyboard input (general)

Key names use the Linux keysym set. Common keys:

```bash
virsh --connect qemu:///system send-key virtue-win11 --codeset linux KEY_ENTER
virsh --connect qemu:///system send-key virtue-win11 --codeset linux KEY_LEFTMETA  # Windows key
virsh --connect qemu:///system send-key virtue-win11 --codeset linux KEY_ESC
```

## Notes

- The VM is primarily a build machine. See `remote-windows-build.sh` for full build options.
- The Virtue client is not pre-installed; build and install the MSIX before testing client behavior.
- The `--connect qemu:///system` flag is required because the VMs are owned by the system daemon, not the user session.
- sccache is installed on the VM and speeds up Rust rebuilds significantly.
- Sleep, hibernate, and display-off timeouts have been set to "never" (`powercfg /change
standby-timeout-ac 0`, `monitor-timeout-ac 0`, `hibernate-timeout-ac 0`, and their `-dc`
  counterparts, plus `powercfg /hibernate off`) so the VM doesn't suspend or lock mid-task.
  These are OS settings, not VM config, so they only need reapplying if the VM disk is reset
  or reprovisioned.
