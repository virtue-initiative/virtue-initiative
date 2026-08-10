# Device -> api/hash-server integration smoke test (Windows).
#
# Boots the api worker locally against a fresh D1 database (the api's own
# D1-backed /hash routes stand in for the standalone Rust hash-server in
# local dev -- see api/src/lib/hash-server.ts and scripts/launch.sh), seeds
# the deterministic dev account, builds and runs a small `virtue-windows-ci-runner`
# binary that drives the real `virtue_windows` monitoring code in-process, then
# asserts that hashes and batches actually landed in the database.
#
# Unlike Linux (`virtue login` CLI) and macOS (a daemon binary + an IPC-socket
# login helper), the Windows client has no standalone daemon process or CLI --
# `virtue_windows` is purely a cdylib the WinUI app loads via P/Invoke, and
# monitoring/login both happen as in-process calls against a background
# thread that same process spawns (see `RustInteropClient.cs` /
# `SessionViewModel.cs` for the app's own call sequence: Initialize ->
# StartMonitoring -> Login). `ci-runner.rs` reproduces that sequence directly
# against the `virtue-windows` library, then blocks for a fixed run window so
# the monitor's background thread can actually capture/hash/batch/upload
# before the process exits, and exits on its own once that window elapses --
# there's no separate daemon process to start, log in to, and kill.
#
# GitHub's windows-latest runners have a real interactive desktop session, so
# GDI screen capture (`capture.rs`) produces a genuine screenshot with no
# permission prompt and no virtual-display trick needed (unlike Linux's Xvfb
# or macOS's Screen Recording consent).
#
# Usage: .\client\windows\scripts\integration-test.ps1
#
# Requires: bun, cargo, all on PATH. Windows only.

$ErrorActionPreference = "Stop"

if (-not $IsWindows) {
    Write-Error "integration-test: this script only runs on Windows"
    exit 1
}

foreach ($cmd in @("bun", "cargo")) {
    if (-not (Get-Command $cmd -ErrorAction SilentlyContinue)) {
        Write-Error "integration-test: missing required command '$cmd' on PATH"
        exit 1
    }
}

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$Root = (Resolve-Path (Join-Path $ScriptDir "..\..\..")).Path
$ClientDir = Join-Path $Root "client"
$ApiDir = Join-Path $Root "api"

$DevEmail = "dev@dev.com"
$DevPassword = "devpassword"
$DeviceName = "ci-integration-test-$PID"

# capture_interval_seconds has a 15s floor enforced by client/core/src/config.rs.
$CaptureIntervalSeconds = 15
$BatchWindowSeconds = 15
$RunDurationSeconds = 60

$LogDir = Join-Path ([System.IO.Path]::GetTempPath()) "virtue-windows-ci-logs-$([guid]::NewGuid())"
New-Item -ItemType Directory -Path $LogDir | Out-Null
$ApiOutLog = Join-Path $LogDir "api.out.log"
$ApiErrLog = Join-Path $LogDir "api.err.log"
$RunnerOutLog = Join-Path $LogDir "runner.out.log"
$RunnerErrLog = Join-Path $LogDir "runner.err.log"

# Isolated PROGRAMDATA for the client under test only -- NOT set for the
# whole script's environment. `ClientPaths::discover()` resolves everything
# off PROGRAMDATA (see client/windows/src/config.rs), so overriding it just
# for the runner process isolates %ProgramData%\Virtue the same way Linux
# isolates XDG_CONFIG_HOME/XDG_STATE_HOME and macOS isolates $HOME --
# without touching a real local `virtue` install.
$TmpProgramData = Join-Path ([System.IO.Path]::GetTempPath()) "virtue-windows-ci-$([guid]::NewGuid())"
New-Item -ItemType Directory -Path $TmpProgramData | Out-Null

$ApiProc = $null
$RunnerProc = $null
$ExitCode = 0

function Stop-ProcessTree {
    param([System.Diagnostics.Process]$Process)
    if ($null -eq $Process) { return }
    try {
        if (-not $Process.HasExited) {
            & taskkill /PID $Process.Id /T /F 2>$null | Out-Null
        }
    } catch {
        # already exited
    }
}

try {
    Write-Host "== Picking a free port for the api dev server =="
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
    $listener.Start()
    $ApiPort = $listener.LocalEndpoint.Port
    $listener.Stop()
    $ApiBaseUrl = "http://localhost:$ApiPort"

    Write-Host "== Setting up api/ local dev environment (port $ApiPort) =="
    Push-Location $ApiDir
    try {
        $devVarsPath = Join-Path $ApiDir ".dev.vars"
        if (-not (Test-Path $devVarsPath)) {
            Copy-Item (Join-Path $ApiDir ".dev.vars.example") $devVarsPath
        }
        if (-not (Test-Path (Join-Path $ApiDir "node_modules"))) {
            bun install
            if ($LASTEXITCODE -ne 0) { throw "bun install failed" }
        }
        bun run db:migrate:local
        if ($LASTEXITCODE -ne 0) { throw "db:migrate:local failed" }
    } finally {
        Pop-Location
    }

    Write-Host "== Starting api dev server =="
    $ApiProc = Start-Process -FilePath "bun" `
        -ArgumentList @("run", "dev", "--", "--port", "$ApiPort", "--var", "HASH_SERVER_URL:$ApiBaseUrl/api") `
        -WorkingDirectory $ApiDir `
        -RedirectStandardOutput $ApiOutLog `
        -RedirectStandardError $ApiErrLog `
        -NoNewWindow -PassThru

    Write-Host "== Waiting for api dev server to become ready =="
    $ready = $false
    for ($i = 0; $i -lt 60; $i++) {
        try {
            Invoke-WebRequest -Uri "$ApiBaseUrl/" -UseBasicParsing -TimeoutSec 2 | Out-Null
            $ready = $true
            break
        } catch {
            Start-Sleep -Seconds 1
        }
    }
    if (-not $ready) {
        throw "integration-test: api dev server did not become ready in time"
    }

    Write-Host "== Seeding dev user =="
    bun run (Join-Path $Root "scripts\seed-dev-user.mjs")
    if ($LASTEXITCODE -ne 0) { throw "seed-dev-user failed" }

    Write-Host "== Building virtue-windows-ci-runner =="
    Push-Location $ClientDir
    try {
        cargo build --target x86_64-pc-windows-msvc -p virtue-windows --bin virtue-windows-ci-runner
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    } finally {
        Pop-Location
    }
    $RunnerBin = Join-Path $ClientDir "target\x86_64-pc-windows-msvc\debug\virtue-windows-ci-runner.exe"

    Write-Host "== Running the client (init/login/capture/batch) =="
    $oldProgramData = $env:PROGRAMDATA
    $env:PROGRAMDATA = $TmpProgramData
    try {
        $RunnerProc = Start-Process -FilePath $RunnerBin `
            -ArgumentList @(
                "--api-base-url", $ApiBaseUrl,
                "--email", $DevEmail,
                "--password", $DevPassword,
                "--device-name", $DeviceName,
                "--capture-interval-seconds", $CaptureIntervalSeconds,
                "--batch-window-seconds", $BatchWindowSeconds,
                "--run-duration-seconds", $RunDurationSeconds
            ) `
            -RedirectStandardOutput $RunnerOutLog `
            -RedirectStandardError $RunnerErrLog `
            -NoNewWindow -PassThru
    } finally {
        $env:PROGRAMDATA = $oldProgramData
    }

    $completed = $RunnerProc.WaitForExit(($RunDurationSeconds + 60) * 1000)
    if (-not $completed) {
        throw "integration-test: virtue-windows-ci-runner did not finish within the expected window"
    }
    if ($RunnerProc.ExitCode -ne 0) {
        throw "integration-test: virtue-windows-ci-runner exited with code $($RunnerProc.ExitCode)"
    }

    Write-Host "== Verifying database state =="

    function Get-D1QueryCount {
        param([string]$Sql)
        Push-Location $ApiDir
        try {
            $json = (& bun run wrangler d1 execute staging-app-db --local --env staging --json --command $Sql) -join "`n"
        } finally {
            Pop-Location
        }
        $data = $json | ConvertFrom-Json
        if ($null -ne $data -and $data.Count -gt 0 -and $data[0].results.Count -gt 0) {
            return [int]$data[0].results[0].c
        }
        return 0
    }

    # hash_states.count is a rolling per-batch-window counter, not a
    # cumulative total: api/src/routes/device-only.ts resets it to 0 after
    # every successful POST /d/batch (see hashReset() there), so with our
    # short batch window it can legitimately read 0 moments after a hash was
    # ingested. hashed_at is never touched by that reset (see
    # localHashReset in api/src/lib/hash-server.ts), so it's the durable
    # signal that at least one hash was ever ingested.
    #
    # A `wrangler d1 execute --local` CLI process reading the same on-disk D1
    # state can also lag slightly behind Miniflare's in-process view right
    # after a write, so retry for a few seconds instead of asserting on one
    # snapshot.
    $fail = $true
    $DeviceCount = 0
    $HashCount = 0
    $BatchCount = 0
    for ($i = 0; $i -lt 15; $i++) {
        $DeviceCount = Get-D1QueryCount "SELECT COUNT(*) as c FROM devices WHERE name = '$DeviceName'"
        $HashCount = Get-D1QueryCount "SELECT COUNT(*) as c FROM hash_states hs JOIN devices d ON d.id = hs.device_id WHERE d.name = '$DeviceName' AND hs.hashed_at IS NOT NULL"
        $BatchCount = Get-D1QueryCount "SELECT COUNT(*) as c FROM batches b JOIN devices d ON d.id = b.device_id WHERE d.name = '$DeviceName'"

        Write-Host "device rows: $DeviceCount, ever-hashed: $HashCount, batch rows: $BatchCount"

        if ($DeviceCount -ge 1 -and $HashCount -ge 1 -and $BatchCount -ge 1) {
            $fail = $false
            break
        }
        Start-Sleep -Seconds 2
    }

    if ($fail) {
        if ($DeviceCount -lt 1) { Write-Host "integration-test: expected a devices row for '$DeviceName'" }
        if ($HashCount -lt 1) { Write-Host "integration-test: expected a hash_states row with hashed_at set for '$DeviceName'" }
        if ($BatchCount -lt 1) { Write-Host "integration-test: expected at least one batch row for '$DeviceName'" }

        Push-Location $ApiDir
        try {
            Write-Host "--- devices (raw) ---"
            bun run wrangler d1 execute staging-app-db --local --env staging --command "SELECT hex(id) as id, name FROM devices"
            Write-Host "--- hash_states (raw) ---"
            bun run wrangler d1 execute staging-app-db --local --env staging --command "SELECT hex(device_id) as device_id, count, hashed_at FROM hash_states"
            Write-Host "--- batches (raw) ---"
            bun run wrangler d1 execute staging-app-db --local --env staging --command "SELECT hex(device_id) as device_id, COUNT(*) as n FROM batches GROUP BY device_id"
        } finally {
            Pop-Location
        }

        throw "integration-test: database verification failed"
    }

    Write-Host "== Integration test passed =="
} catch {
    $ExitCode = 1
    Write-Host $_
    Write-Host "=== api stdout ==="
    Get-Content $ApiOutLog -ErrorAction SilentlyContinue
    Write-Host "=== api stderr ==="
    Get-Content $ApiErrLog -ErrorAction SilentlyContinue
    Write-Host "=== runner stdout ==="
    Get-Content $RunnerOutLog -ErrorAction SilentlyContinue
    Write-Host "=== runner stderr ==="
    Get-Content $RunnerErrLog -ErrorAction SilentlyContinue
} finally {
    Stop-ProcessTree $RunnerProc
    Stop-ProcessTree $ApiProc
    Remove-Item -Recurse -Force $TmpProgramData -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force $LogDir -ErrorAction SilentlyContinue
}

exit $ExitCode
