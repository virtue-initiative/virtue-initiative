# Windows VM Setup (Single VM for Build + Test)

This guide assumes:

- Linux host with `virt-manager` / libvirt.
- One Windows VM used for both build and GUI testing.
- Repo path on Linux: `/home/jeff/code/virtue-initiative`.

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

Note: this removes VM disk storage. Keep ISO files outside VM-managed storage and
recheck paths before recreating.

Set resources (good starting point for 12 GB host RAM): 4608 MiB RAM, 3 vCPUs.
Use virt-manager hardware settings if you want to tune this later.

Start VM:

```bash
virsh start win11
```

### Installer console checklist

- Use graphical console (`virt-viewer win11` or virt-manager `Display Spice`).
- Do **not** use `virsh console win11` for Windows install (serial text console).
- If you see text like `Ubuntu 24.04 PC (Q35 + ICH9)` at the top, that is
  firmware/machine labeling, not proof of wrong ISO.
- In boot menu, pick the UEFI DVD entry for the Windows ISO.
- On first prompt, press a key for `Press any key to boot from CD/DVD`.
- On later reboots, do **not** press a key (let it continue from disk).

Complete Windows setup in the VM UI and sign in.

### Enable host <-> guest copy/paste (SPICE agent)

If clipboard sharing does not work in virt-manager/virt-viewer, install SPICE
guest tools inside Windows (PowerShell as Administrator):

```powershell
$exe="$env:TEMP\spice-guest-tools.exe"
Invoke-WebRequest -Uri "https://www.spice-space.org/download/windows/spice-guest-tools/spice-guest-tools-latest.exe" -OutFile $exe
Start-Process -FilePath $exe -Wait
Restart-Computer
```

After reboot, reconnect to the VM (`virt-viewer win11` or virt-manager).
Clipboard copy/paste should now work between host and guest.

### If no disk appears in Windows setup

When installer says no disk is available:

1. Click `Load driver` -> `Browse`.
2. Open VirtIO CD drive.
3. Use `amd64\w11` first.
4. Select/install storage driver shown there (`vioscsi` or `viostor`).
5. Return to disk list and click `Refresh`.

Notes:

- `Red Hat` in driver names is expected for VirtIO.
- Use `amd64` (not `x86` or `ARM64`).
- If `w11` does not work, try `amd64\w10`.

### If OOBE asks for a network driver

When first-login setup requires internet but no adapter is detected:

1. Click `Install driver` / `Load driver`.
2. Open VirtIO CD drive.
3. Use `NetKVM\w11\amd64` first.
4. If needed, try `NetKVM\w10\amd64`.
5. Select the NIC driver and continue.

`Red Hat` labeling is expected for VirtIO drivers.

Optional offline fallback:

1. Press `Shift+F10`.
2. Try direct local-account flow first:

```cmd
start ms-cxh:localonly
```

3. If that does not open local account setup, run:

```cmd
OOBE\BYPASSNRO
```

4. After reboot, choose offline setup (`I don't have internet`).
5. If the offline option is still missing, temporarily unplug NIC in
   virt-manager and continue OOBE, then reconnect after first login.

## 2) Serve bootstrap script from Linux

In a Linux terminal:

```bash
cd /home/jeff/code/virtue-initiative/client/windows/scripts
python3 -m http.server 8765 --bind 0.0.0.0
```

Keep this running temporarily.

In commands below, replace `<HOST_IP>` with your Linux host IP reachable from the VM.

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
.\bootstrap-win11-build-vm.ps1 -AuthorizedKey $pub -ApiBaseUrl "http://<HOST_IP>:8787"
```

If you are not setting SSH key right now:

```powershell
Set-ExecutionPolicy -Scope Process Bypass -Force
.\bootstrap-win11-build-vm.ps1 -ApiBaseUrl "http://<HOST_IP>:8787"
```

Reboot the VM once after bootstrap finishes.

## 4) Add SSH host alias on Linux

Find VM IP:

```bash
virsh domifaddr win11 --source agent
virsh domifaddr win11 --source lease
```

Use the IPv4 address shown for the VM NIC (strip CIDR suffix like `/24`).

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

## 5) Run first smoke build from Linux

```bash
cd /home/jeff/code/virtue-initiative
./client/windows/scripts/remote-windows-build.sh --build-host win11 --mode smoke
```

The full run log is saved on Linux under:

- `client/windows/dist/remote-logs/`

## 6) Build installer from Linux

```bash
./client/windows/scripts/remote-windows-build.sh \
  --build-host win11 \
  --mode installer \
  --profile Debug \
  --version 0.1.0-dev
```

By default, the installer remains on the Windows VM at:

- `C:\virtue-build\src\client\windows\dist\virtue-windows-installer-0.1.0-dev.exe`

If you also want a Linux copy:

```bash
./client/windows/scripts/remote-windows-build.sh \
  --build-host win11 \
  --mode installer \
  --profile Debug \
  --version 0.1.0-dev \
  --copy-installer-to-linux
```

That copies to:

- `client/windows/dist/remote/virtue-windows-installer-0.1.0-dev.exe`
