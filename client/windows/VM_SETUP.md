# Windows VM Setup (Single VM for Build + Test)

This guide assumes:

- Linux host with `virt-manager` / libvirt
- one Windows VM used for both build and GUI testing
- repo path on Linux: `/home/jeff/code/virtue-initiative`

The Windows build path now targets:

- Rust backend artifact (`virtue_windows.dll`)
- C# WinUI 3 resident app in `client/windows/`
- MSIX output built remotely over SSH

## 1) Create or reset the VM

If you already have a `win11` VM and want to reuse it, skip to step 2.

Create from ISO:

```bash
virt-install \
  --name win11 \
  --memory 4608 \
  --vcpus 3 \
  --cpu host-passthrough \
  --os-variant win11 \
  --machine q35 \
  --disk size=120,bus=virtio \
  --cdrom ~/isos/Win11_English_x64.iso \
  --disk path=~/isos/virtio-win.iso,device=cdrom \
  --network network=default,model=virtio \
  --graphics spice \
  --video virtio \
  --boot uefi \
  --noautoconsole
```

If you need to remove and recreate:

```bash
virsh destroy win11
virsh undefine win11 --nvram --remove-all-storage
```

Set resources (good starting point for a 12 GB host): 4608 MiB RAM, 3 vCPUs.

Start VM:

```bash
virsh start win11
```

### Installer console checklist

- Use a graphical console (`virt-viewer win11` or virt-manager `Display Spice`).
- Do not use `virsh console win11` for Windows installation.
- Use the UEFI DVD entry for the Windows ISO.
- Press a key only on the first `Press any key to boot from CD/DVD` prompt.

Complete Windows setup in the VM UI and sign in.

### If no disk appears in Windows setup

When the installer says no disk is available:

1. Click `Load driver` -> `Browse`.
2. Open the VirtIO CD drive.
3. Try `amd64\w11` first.
4. Install the storage driver shown there.
5. Return to disk list and click `Refresh`.

### If OOBE asks for a network driver

When first-login setup requires internet but no adapter is detected:

1. Click `Install driver` / `Load driver`.
2. Open the VirtIO CD drive.
3. Try `NetKVM\w11\amd64` first.
4. If needed, try `NetKVM\w10\amd64`.

## 2) Serve bootstrap script from Linux

In a Linux terminal:

```bash
cd /home/jeff/code/virtue-initiative/client/windows/scripts
python3 -m http.server 8765 --bind 0.0.0.0
```

Keep this running temporarily.

In the commands below, replace `<HOST_IP>` with the Linux host IP reachable from the VM.

## 3) Run bootstrap script in Windows (as Administrator)

In the Windows VM (PowerShell as Administrator):

```powershell
cd $env:TEMP
Invoke-WebRequest -Uri "http://<HOST_IP>:8765/bootstrap-win11-build-vm.ps1" -OutFile ".\bootstrap-win11-build-vm.ps1"
```

If you want SSH key auth immediately, paste your Linux public key:

```powershell
$pub = "ssh-ed25519 AAAA... your-key-comment"
```

Run bootstrap:

```powershell
Set-ExecutionPolicy -Scope Process Bypass -Force
.\bootstrap-win11-build-vm.ps1 -AuthorizedKey $pub -ApiBaseUrl "http://<HOST_IP>:8787" -CaptureIntervalSeconds 10 -BatchWindowSeconds 30
```

If you are not setting SSH key right now:

```powershell
Set-ExecutionPolicy -Scope Process Bypass -Force
.\bootstrap-win11-build-vm.ps1 -ApiBaseUrl "http://<HOST_IP>:8787" -CaptureIntervalSeconds 10 -BatchWindowSeconds 30
```

The bootstrap script installs:

- WinGet / App Installer bootstrap support
- OpenSSH server
- Git
- Rust MSVC toolchain + clippy
- .NET 8 SDK
- Visual Studio Build Tools with MSBuild, C++ tools, managed desktop tools, and Windows SDK
- optional `sccache`

You do not need to preinstall `winget` manually anymore. The bootstrap script now tries the Microsoft-supported PowerShell module flow (`Microsoft.WinGet.Client` + `Repair-WinGetPackageManager -AllUsers`) before it uses `winget` to install the rest of the toolchain.

Reboot the VM once after bootstrap completes.

## 4) Add SSH host alias on Linux

Find the VM IP:

```bash
virsh domifaddr win11 --source agent
virsh domifaddr win11 --source lease
```

Add/update `~/.ssh/config`:

```sshconfig
Host win11
  HostName <vm-ip>
  User <windows-username>
```

Test:

```bash
ssh win11 'echo connected'
```

## 5) Run the first remote smoke build

```bash
cd /home/jeff/code/virtue-initiative
./client/windows/scripts/remote-windows-build.sh \
  --build-host win11 \
  --mode smoke
```

This remote smoke run validates:

- Rust build + clippy for `virtue-core`
- Rust build + clippy for `virtue-windows`
- WinUI dependency restore
- managed core build
- managed tests
- WinUI app compile without packaging

The full run log is saved locally under:

- `client/windows/dist/remote-logs/`

## 6) Build the Windows MSIX package from Linux

```bash
./client/windows/scripts/remote-windows-build.sh \
  --build-host win11 \
  --mode msix \
  --profile Debug \
  --version 0.0.8-dev
```

By default, the package remains on the Windows VM at:

- `C:\virtue-build\src\client\windows\dist\virtue-windows-0.0.8-dev.msix`
- `C:\virtue-build\src\client\windows\dist\virtue-windows-0.0.8-dev-setup.zip`

## 7) Optional direct Windows build checks

Inside the VM:

```powershell
cd C:\virtue-build\src\client
cargo build --target x86_64-pc-windows-msvc -p virtue-core
cargo build --target x86_64-pc-windows-msvc -p virtue-windows
dotnet test .\windows\Virtue.WindowsApp.Tests\Virtue.WindowsApp.Tests.csproj -c Debug
.\windows\scripts\build-msix.ps1 -Profile Debug -Version 0.0.8-dev
```
