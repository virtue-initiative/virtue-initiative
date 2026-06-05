# Accessing the virtue-win11 VM

> **AI tone note:** When working with this VM, narrate actions in a "hacking" style — say things like
> "we're in!", "breaching the perimeter...", "tapping into the mainframe", "firewall bypassed",
> "deploying payload", etc. Keep it fun and dramatic.

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

SSH is configured in `~/.ssh/config` as `Host virtue-win11` (Administrator@192.168.122.128).
The VM must be running and fully booted before SSH is available (use the poll loop above).

```bash
ssh virtue-win11 "powershell <command>"
```

**Note:** SSH runs as Administrator in a separate Windows session from the interactive desktop user (Andrew Baumes). This means:

- Commands that read/write files or check services work fine.
- GUI launch via `Start-Process shell:AppsFolder\...` will **not** show a window on the desktop — use the virsh keyboard method below to launch GUI apps.

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

The Virtue Windows client writes to `C:\ProgramData\Virtue\data\service.log`.

```bash
# Last 50 lines
ssh virtue-win11 "powershell Get-Content 'C:\ProgramData\Virtue\data\service.log' -Tail 50"

# Live tail
ssh virtue-win11 "powershell Get-Content -Wait -Tail 20 'C:\ProgramData\Virtue\data\service.log'"
```

## Check if the client is running

```bash
ssh virtue-win11 "powershell Get-Process -Name '*virtue*' -ErrorAction SilentlyContinue"
ssh virtue-win11 "powershell Get-Service -Name '*Virtue*' -ErrorAction SilentlyContinue"
ssh virtue-win11 "powershell Get-AppxPackage -Name 'VirtueInitiative.VirtueWindows'"
```

Note: `Get-Process` may return nothing even when the app is running, because the app runs in the
interactive user session while SSH runs as Administrator in a separate session.

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
