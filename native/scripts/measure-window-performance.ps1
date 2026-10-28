#requires -Version 7
# Fixed visible-window protocol. Both binaries must contain the same diagnostic
# instrumentation. Samples are main-process metrics; child Shell CPU is excluded.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$BaselineExe,
    [Parameter(Mandatory)][string]$CandidateExe,
    [Parameter(Mandatory)][string]$OutputDirectory,
    [ValidateSet('idle', 'mixed')][string[]]$Loads = @('idle', 'mixed'),
    [ValidateRange(8, 1000)][int]$LoadIntervalMs = 50,
    [ValidateSet(1, 8)][int]$Panes = 1,
    [ValidateRange(0, 20000)][int]$HistoryLines = 0,
    [switch]$Recycle,
    [switch]$ThreadCpu
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$outputRoot = [IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $outputRoot) { throw 'Use a new output directory so prior samples are retained.' }
if (($HistoryLines -or $Recycle) -and $Panes -ne 8) { throw 'History/recycle diagnostics require eight panes.' }
$executables = @{
    baseline = (Resolve-Path -LiteralPath $BaselineExe).Path
    candidate = (Resolve-Path -LiteralPath $CandidateExe).Path
}
$lock = $null
$oldEnvironment = @{}
foreach ($name in @('TIDETERM_MEMORY_PROBE_SECONDS', 'TIDETERM_WINDOW_PERF', 'TIDETERM_WINDOW_PERF_LOAD',
    'TIDETERM_WINDOW_PERF_INTERVAL_MS', 'TIDETERM_WINDOW_PERF_PANES', 'TIDETERM_WINDOW_PERF_HISTORY_LINES',
    'TIDETERM_WINDOW_PERF_RECYCLE', 'TIDETERM_WINDOW_EVENT_PROFILE', 'TEMP', 'TMP')) {
    $oldEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
}
try {
    $lock = [IO.File]::Open((Join-Path $repo 'native/target/gui-validation.lock'),
        [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    if (Get-Process -Name native -ErrorAction SilentlyContinue) {
        throw 'A native process is already running; do not overlap GUI validation.'
    }
    New-Item -ItemType Directory -Path $outputRoot | Out-Null
    $taskTemp = Join-Path $repo '.tmp/window-perf'
    New-Item -ItemType Directory -Force -Path $taskTemp | Out-Null
    $env:TEMP = $taskTemp
    $env:TMP = $taskTemp
    $env:TIDETERM_MEMORY_PROBE_SECONDS = '35'
    $env:TIDETERM_WINDOW_PERF = '1'
    $env:TIDETERM_WINDOW_PERF_INTERVAL_MS = "$LoadIntervalMs"
    $env:TIDETERM_WINDOW_PERF_PANES = "$Panes"
    $env:TIDETERM_WINDOW_PERF_HISTORY_LINES = "$HistoryLines"
    $env:TIDETERM_WINDOW_PERF_RECYCLE = if ($Recycle) { '1' } else { '0' }
    $env:TIDETERM_WINDOW_EVENT_PROFILE = $null
    $metadata = [ordered]@{
        StartedUtc = [DateTime]::UtcNow.ToString('o')
        LockOwner = $PID
        LockPath = Join-Path $repo 'native/target/gui-validation.lock'
        BaselineSHA256 = (Get-FileHash -LiteralPath $executables.baseline -Algorithm SHA256).Hash
        CandidateSHA256 = (Get-FileHash -LiteralPath $executables.candidate -Algorithm SHA256).Hash
        DriverSHA256 = (Get-FileHash -LiteralPath $PSCommandPath -Algorithm SHA256).Hash
        ResourceSamplerSHA256 = (Get-FileHash -LiteralPath (Join-Path $PSScriptRoot 'measure-startup.ps1') -Algorithm SHA256).Hash
        SampleSeconds = @(6, 16, 26)
        ProbeSeconds = 35
        SamplingOrigin = 'ForegroundWindowReady'
        ActivationTimeoutSeconds = 30
        VisibleWindowActivationTimeoutSeconds = 2
        Loads = $Loads
        LoadIntervalMs = $LoadIntervalMs
        Panes = $Panes
        HistoryLines = $HistoryLines
        Recycle = [bool]$Recycle
        ThreadCpu = [bool]$ThreadCpu
        RequireForeground = $true
        ForegroundProtection = 'LockSetForegroundWindow'
        WindowIdentitySource = 'ProbeReadyMarker'
        DllInitializationRetries = 1
    }
    $metadata | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $outputRoot 'metadata.json') -Encoding utf8
    $referenceGeometry = $null
    foreach ($load in $Loads) {
        $env:TIDETERM_WINDOW_PERF_LOAD = $load
        foreach ($sample in 0..3) {
            $order = if ($sample -eq 2) { @('candidate', 'baseline') } else { @('baseline', 'candidate') }
            foreach ($variant in $order) {
                if (Get-Process -Name native -ErrorAction SilentlyContinue) {
                    throw 'A native process appeared between samples; stop overlapping validation.'
                }
                $kind = if ($sample -eq 0) { 'warmup' } else { "sample-$sample" }
                $runRoot = Join-Path $outputRoot "$load-$variant-$kind"
                New-Item -ItemType Directory -Path $runRoot | Out-Null
                Write-Output "BEGIN load=$load variant=$variant kind=$kind"
                for ($attempt = 1; $attempt -le 2; ++$attempt) {
                    $attemptRoot = Join-Path $runRoot "attempt-$attempt"
                    try {
                        $rows = @(& (Join-Path $PSScriptRoot 'measure-startup.ps1') -ExePath $executables[$variant] `
                            -ArgumentList '--memory-probe --full --c8-selftest' -Visible `
                            -SampleSeconds @(6, 16, 26) -OutputDirectory $attemptRoot -ThreadCpu:$ThreadCpu -RequireForeground)
                        foreach ($name in @('process.stdout.log', 'process.stderr.log', 'process-metadata.json')) {
                            Copy-Item -LiteralPath (Join-Path $attemptRoot $name) -Destination (Join-Path $runRoot $name)
                        }
                        break
                    } catch {
                        $_.Exception.Message | Set-Content -LiteralPath (Join-Path $attemptRoot 'failure.txt') -Encoding utf8
                        if ($attempt -eq 2 -or $_.Exception.Message -ne 'Diagnostic process exited during activation: -1073741502') { throw }
                        Write-Output "RETRY pre-window DLL initialization failure; retained $attemptRoot"
                    }
                }
                $rows | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $runRoot 'resources.json') -Encoding utf8
                if ($rows.Count -ne 3 -or @($rows | Where-Object { -not $_.ClientWidthPx -or -not $_.ClientHeightPx }).Count) {
                    throw "Invalid window sample: $runRoot"
                }
                foreach ($row in $rows) {
                    if ($row.WindowIdentitySource -ne 'ProbeReadyMarker') { throw "Missing explicit probe window identity: $runRoot" }
                    if ($row.Minimized -ne $false) { throw "The diagnostic window is minimized or its state is unavailable: $runRoot" }
                    $geometry = "$($row.ClientWidthPx)x$($row.ClientHeightPx)@$($row.WindowDpi)"
                    if (-not $referenceGeometry) { $referenceGeometry = $geometry }
                    if ($geometry -ne $referenceGeometry) { throw "Window geometry changed: $geometry expected $referenceGeometry" }
                }
                $profile = @(Get-Content -LiteralPath (Join-Path $runRoot 'process.stdout.log') | Where-Object { $_ -match '^\[window-perf\] group=' })
                if ($profile.Count -ne 4 -or @($profile | Where-Object { $_ -match 'count=0\s|dropped=[1-9]' }).Count) {
                    throw "Missing or truncated GUI timings: $runRoot"
                }
                if (-not (Select-String -LiteralPath (Join-Path $runRoot 'process.stdout.log') -Pattern '^\[window-perf\] foreground_handoff=1$')) {
                    throw "Foreground probe did not hand activation back to its launcher: $runRoot"
                }
                foreach ($phase in @('lock', 'unlock')) {
                    if (-not (Select-String -LiteralPath (Join-Path $runRoot 'process.stdout.log') -Pattern "^\[window-perf\] foreground_${phase}=1$")) {
                        throw "Foreground protection did not complete ${phase}: $runRoot"
                    }
                }
                if ($Panes -eq 8) {
                    $log = Get-Content -LiteralPath (Join-Path $runRoot 'process.stdout.log')
                    if (@($log | Where-Object { $_ -match '^\[window-scenario\] pane=\d+ width=[1-9]\d* height=[1-9]\d* cols=[1-9]\d* rows=[1-9]\d*$' }).Count -ne 8) {
                        throw "Missing eight usable pane geometries: $runRoot"
                    }
                    if ($Recycle -and -not ($log -match '^\[window-scenario\] phase=closed panes=1$')) {
                        throw "Pane close scenario did not complete: $runRoot"
                    }
                }
                if ($load -eq 'mixed' -and -not (Select-String -LiteralPath (Join-Path $runRoot 'process.stdout.log') -Pattern '^\[window-perf\] load_count=[1-9]\d* payload_bytes=[1-9]\d* total_bytes=[1-9]\d*$')) {
                    throw "Missing delivered output count: $runRoot"
                }
                Write-Output "END load=$load variant=$variant kind=$kind cpu=$($rows[-1].CPUOneCorePercent) private=$($rows[-1].PrivateMB)"
            }
        }
    }
} finally {
    foreach ($name in $oldEnvironment.Keys) {
        [Environment]::SetEnvironmentVariable($name, $oldEnvironment[$name], 'Process')
    }
    if ($lock) { $lock.Dispose() }
}
