#requires -Version 7
<# Measure one fresh GUI process with the same sampling points for TideTerm or WindTerm.
   TaskManagerMB matches the Processes tab's Memory column (private working set).
   Example: pwsh ./measure-startup.ps1 -ArgumentList '--memory-probe --full'
            pwsh ./measure-startup.ps1 -ArgumentList '--memory-probe --chrome-terminal'
            pwsh ./measure-startup.ps1 -ArgumentList '--memory-probe --full-no-paint'
            pwsh ./measure-startup.ps1 -ArgumentList '--memory-probe --full-blank-paint'
            pwsh ./measure-startup.ps1 -ArgumentList '--memory-probe --terminal'
            pwsh ./measure-startup.ps1 -ArgumentList '--memory-probe --tab-nopump'
            pwsh ./measure-startup.ps1 -ArgumentList '--memory-probe --view-only'
            pwsh ./measure-startup.ps1 -ArgumentList '--memory-probe --shell-only'
            pwsh ./measure-startup.ps1 -ArgumentList '--memory-probe --render'
            pwsh ./measure-startup.ps1 -ArgumentList '--memory-probe --ascii'
            pwsh ./measure-startup.ps1 -ArgumentList '--memory-probe --cjk'
            # 中文进屏花多少钱（窗格单独摆出来，2026-09-19 口径 11.9 / 12.0 / 20.5 MB）：
            pwsh ./measure-startup.ps1 -ArgumentList '--memory-probe --solo-ascii'
            pwsh ./measure-startup.ps1 -ArgumentList '--memory-probe --solo-cjk'
            pwsh ./measure-startup.ps1 -ArgumentList '--memory-probe --solo-raster-cjk'
            # 每多一格花多少钱（主窗 + 8 格，2026-09-19 口径 33 / 34 / 27.5 MB）：
            pwsh ./measure-startup.ps1 -ArgumentList '--memory-probe --pane8-hidden'
            pwsh ./measure-startup.ps1 -ArgumentList '--memory-probe --pane8-paint'
            pwsh ./measure-startup.ps1 -ArgumentList '--memory-probe --pane8-grid'
            pwsh ./measure-startup.ps1 -ExePath 'D:\apps\WindTerm\WindTerm.exe' -ArgumentList ''
            # 同机同窗口，加 -Visible 才是"用户真正看到的那个最大化窗口"的口径。
            pwsh ./measure-startup.ps1 -ArgumentList '--window' -Visible
   CPUOneCorePercent is CPU time / elapsed wall time, without logical-core normalization.
   This reports the main process only; a spawned shell is excluded. #>
param(
    [string]$ExePath = (Join-Path $PSScriptRoot '../target/release/native.exe'),
    [string]$ArgumentList = '--window',
    [ValidateSet('windows', 'offscreen')][string]$QtPlatform = 'windows',
    # 默认 Hidden：窗口拿不到 expose，也就没有 DWM 重定向表面（本机实测少一整块
    # 15 MB 私有提交）。-Visible 才等价于用户真正看到的那个最大化窗口。
    [switch]$Visible,
    [ValidateNotNullOrEmpty()][int[]]$SampleSeconds = @(1, 3, 6),
    # Optional target stdout/stderr, kept separate from the structured samples.
    [string]$OutputDirectory,
    [switch]$ThreadCpu,
    [switch]$RequireForeground
)

if ($RequireForeground -and (-not $Visible -or $QtPlatform -ne 'windows')) {
    throw 'Foreground sampling requires -Visible and the Windows backend.'
}
if (($SampleSeconds | Where-Object { $_ -le 0 }).Count -gt 0 -or
    @($SampleSeconds | Sort-Object -Unique).Count -ne $SampleSeconds.Count -or
    ($SampleSeconds -join ',') -ne (($SampleSeconds | Sort-Object) -join ',')) {
    throw 'SampleSeconds must be positive, unique and ascending.'
}

$resolvedExe = (Resolve-Path -LiteralPath $ExePath -ErrorAction Stop).Path
$processName = [IO.Path]::GetFileNameWithoutExtension($resolvedExe)
$oldPath = $env:PATH
$oldPlatform = $env:QT_QPA_PLATFORM
$oldForegroundParent = $env:TIDETERM_WINDOW_PERF_PARENT_PID
$isNativeQt = [IO.Path]::GetFileName($resolvedExe) -eq 'native.exe'
$usesProbeReady = $RequireForeground -and $isNativeQt -and $env:TIDETERM_WINDOW_PERF -eq '1' -and $ArgumentList -like '*--memory-probe*'
if ($usesProbeReady -and -not $OutputDirectory) { throw 'Foreground performance probes require OutputDirectory for the ready_window marker.' }
if ($isNativeQt) {
    if ($env:QT_DIR) {
        $qtBin = Join-Path $env:QT_DIR 'bin'
        if (-not (Test-Path -LiteralPath $qtBin)) { throw "QT_DIR/bin 不存在：$qtBin" }
        $env:PATH = "$qtBin$([IO.Path]::PathSeparator)$oldPath"
    }
    $env:QT_QPA_PLATFORM = $QtPlatform
}
$process = $null
if (-not ('TideTerm.StartupGeometry' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
namespace TideTerm {
    public static class StartupGeometry {
        [StructLayout(LayoutKind.Sequential)]
        public struct Rect { public int Left, Top, Right, Bottom; }
        [DllImport("user32.dll")]
        public static extern bool GetClientRect(IntPtr window, out Rect rect);
        [DllImport("user32.dll")]
        public static extern uint GetDpiForWindow(IntPtr window);
        [DllImport("user32.dll")]
        public static extern bool IsIconic(IntPtr window);
        private delegate bool WindowVisitor(IntPtr window, IntPtr state);
        [DllImport("user32.dll")]
        private static extern bool EnumWindows(WindowVisitor visitor, IntPtr state);
        [DllImport("user32.dll")]
        private static extern uint GetWindowThreadProcessId(IntPtr window, out uint owner);
        [DllImport("user32.dll")]
        private static extern bool IsWindowVisible(IntPtr window);
        public static IntPtr LargestVisibleWindow(int processId) {
            IntPtr best = IntPtr.Zero;
            long area = 0;
            EnumWindows((window, state) => {
                uint owner;
                GetWindowThreadProcessId(window, out owner);
                Rect rect;
                if (owner == processId && IsWindowVisible(window) && GetClientRect(window, out rect)) {
                    long candidate = (long)(rect.Right - rect.Left) * (rect.Bottom - rect.Top);
                    if (candidate > area) { best = window; area = candidate; }
                }
                return true;
            }, IntPtr.Zero);
            return best;
        }
    }
}
'@
}
if ($ThreadCpu -and -not ('TideTerm.StartupThread' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
namespace TideTerm {
    public static class StartupThread {
        [DllImport("user32.dll")]
        public static extern uint GetWindowThreadProcessId(IntPtr window, out uint owner);
    }
}
'@
}
if ($RequireForeground -and -not ('TideTerm.StartupActivation' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
namespace TideTerm {
    public static class StartupActivation {
        [DllImport("user32.dll")]
        public static extern IntPtr GetForegroundWindow();
        [DllImport("user32.dll")]
        public static extern bool SetForegroundWindow(IntPtr window);
        [DllImport("user32.dll")]
        public static extern bool IsWindowVisible(IntPtr window);
        [DllImport("user32.dll")]
        public static extern uint GetWindowThreadProcessId(IntPtr window, out uint owner);
        [DllImport("user32.dll", CharSet = CharSet.Unicode)]
        public static extern int GetClassName(IntPtr window, System.Text.StringBuilder name, int count);
        [DllImport("user32.dll")]
        public static extern IntPtr GetWindow(IntPtr window, uint command);
    }
}
'@
}
function Get-TaskManagerMB([int]$targetProcessId, [string]$name) {
    $counterBase = "\Process($name*)"
    $samples = (Get-Counter -Counter "$counterBase\ID Process", "$counterBase\Working Set - Private" -ErrorAction Stop).CounterSamples
    $idSample = $samples | Where-Object {
        $_.Path.EndsWith('\id process') -and [int]$_.CookedValue -eq $targetProcessId
    } | Select-Object -First 1
    if (-not $idSample) { throw "找不到 PID $targetProcessId 的进程性能计数器" }
    $memorySample = $samples | Where-Object {
        $_.InstanceName -eq $idSample.InstanceName -and $_.Path.EndsWith('\working set - private')
    } | Select-Object -First 1
    if (-not $memorySample) { throw "找不到 PID $targetProcessId 的私有工作集计数器" }
    [math]::Round($memorySample.CookedValue / 1MB, 1)
}
try {
    if ($RequireForeground -and $isNativeQt -and $env:TIDETERM_WINDOW_PERF -eq '1') {
        $env:TIDETERM_WINDOW_PERF_PARENT_PID = "$PID"
    }
    $startParams = @{
        FilePath    = $resolvedExe
        WindowStyle = if ($Visible) { 'Normal' } else { 'Hidden' }
        PassThru    = $true
    }
    if ($ArgumentList) { $startParams.ArgumentList = $ArgumentList }
    if ($OutputDirectory) {
        $outputRoot = [IO.Path]::GetFullPath($OutputDirectory)
        New-Item -ItemType Directory -Force -Path $outputRoot | Out-Null
        $startParams.RedirectStandardOutput = Join-Path $outputRoot 'process.stdout.log'
        $startParams.RedirectStandardError = Join-Path $outputRoot 'process.stderr.log'
    }
    $process = Start-Process @startParams
    if ($OutputDirectory) {
        [pscustomobject]@{ PID = $process.Id; LauncherPID = $PID; StartedUtc = [DateTime]::UtcNow.ToString('o') } |
            ConvertTo-Json | Set-Content -LiteralPath (Join-Path $outputRoot 'process-metadata.json') -Encoding utf8
    }
    $wall = [Diagnostics.Stopwatch]::StartNew()
    $processWall = [Diagnostics.Stopwatch]::StartNew()
    $previousCpuMs = 0.0
    $activationSeconds = $null
    $sampleWindow = [IntPtr]::Zero
    if ($RequireForeground) {
        $activationWindow = [IntPtr]::Zero
        $visibleAt = 0
        while ($wall.ElapsedMilliseconds -lt 30000) {
            $process.Refresh()
            if ($process.HasExited) { throw "Diagnostic process exited during activation: $($process.ExitCode)" }
            if ($usesProbeReady) {
                $window = [IntPtr]::Zero
                $readyLog = Get-Content -LiteralPath (Join-Path $outputRoot 'process.stdout.log') -Raw
                if ($readyLog -match '(?m)^\[window-perf\] ready_window=([1-9]\d*)\r?$') {
                    $window = [IntPtr]::new([long]$Matches[1])
                    $readyOwner = [uint32]0
                    $null = [TideTerm.StartupActivation]::GetWindowThreadProcessId($window, [ref]$readyOwner)
                    if ($readyOwner -ne $process.Id) { throw 'Probe ready_window does not belong to the sampled process.' }
                }
            } else {
                $window = [TideTerm.StartupGeometry]::LargestVisibleWindow($process.Id)
            }
            if ($window -ne [IntPtr]::Zero -and [TideTerm.StartupActivation]::IsWindowVisible($window)) {
                if ($activationWindow -ne $window) {
                    $activationWindow = $window
                    $visibleAt = $wall.ElapsedMilliseconds
                }
                $null = [TideTerm.StartupActivation]::SetForegroundWindow($window)
                if ([TideTerm.StartupActivation]::GetForegroundWindow() -eq $window) { $sampleWindow = $window; break }
                if ($wall.ElapsedMilliseconds - $visibleAt -ge 2000) { break }
            }
            Start-Sleep -Milliseconds 50
        }
        if ($sampleWindow -eq [IntPtr]::Zero) {
            throw "Cannot establish the diagnostic window in the foreground (selected=$window main=$($process.MainWindowHandle) foreground=$([TideTerm.StartupActivation]::GetForegroundWindow()))."
        }
        $activationSeconds = [math]::Round($processWall.Elapsed.TotalSeconds, 3)
        $process.Refresh()
        $previousCpuMs = $process.TotalProcessorTime.TotalMilliseconds
        $wall.Restart()
    }
    $previousWallMs = 0.0
    foreach ($second in $SampleSeconds) {
        $remainingMs = [math]::Ceiling(1000 * $second - $wall.Elapsed.TotalMilliseconds)
        if ($remainingMs -gt 0) { Start-Sleep -Milliseconds $remainingMs }
        $live = Get-Process -Id $process.Id -ErrorAction SilentlyContinue
        if (-not $live) { throw "进程在 $second 秒内退出" }
        # Query counters only at the final sample, so earlier reads do not
        # shift the requested sampling times. Record the actual elapsed time.
        $taskManagerMB = if ($second -eq $SampleSeconds[-1]) { Get-TaskManagerMB $process.Id $processName } else { $null }
        $live.Refresh()
        if ($live.HasExited) { throw "Process exited during sample at $second seconds." }
        $wallMs = $wall.Elapsed.TotalMilliseconds
        $cpuMs = $live.TotalProcessorTime.TotalMilliseconds
        $window = if ($RequireForeground) { $sampleWindow } else { $live.MainWindowHandle }
        $clientRect = [TideTerm.StartupGeometry+Rect]::new()
        $hasGeometry = $window -ne [IntPtr]::Zero -and
            [TideTerm.StartupGeometry]::GetClientRect($window, [ref]$clientRect)
        if ($Visible -and -not $hasGeometry) { throw "Visible process has no measurable client window at $second seconds." }
        if ($RequireForeground -and [TideTerm.StartupActivation]::GetForegroundWindow() -ne $window) {
            $foreground = [TideTerm.StartupActivation]::GetForegroundWindow()
            $foregroundOwner = [uint32]0
            $null = [TideTerm.StartupActivation]::GetWindowThreadProcessId($foreground, [ref]$foregroundOwner)
            $foregroundClass = [Text.StringBuilder]::new(128)
            $null = [TideTerm.StartupActivation]::GetClassName($foreground, $foregroundClass, $foregroundClass.Capacity)
            if ($OutputDirectory) {
                $foregroundRect = [TideTerm.StartupGeometry+Rect]::new()
                $foregroundGeometry = [TideTerm.StartupGeometry]::GetClientRect($foreground, [ref]$foregroundRect)
                [pscustomobject]@{
                    TargetPID = $live.Id; ExpectedWindow = $window.ToInt64()
                    ForegroundWindow = $foreground.ToInt64(); ForegroundPID = $foregroundOwner
                    ForegroundClass = $foregroundClass.ToString()
                    ForegroundOwnerWindow = [TideTerm.StartupActivation]::GetWindow($foreground, 4).ToInt64()
                    ProcessMainWindow = $live.MainWindowHandle.ToInt64()
                    LargestVisibleWindow = [TideTerm.StartupGeometry]::LargestVisibleWindow($live.Id).ToInt64()
                    ExpectedVisible = [TideTerm.StartupActivation]::IsWindowVisible($window)
                    ExpectedClientWidth = $clientRect.Right - $clientRect.Left
                    ExpectedClientHeight = $clientRect.Bottom - $clientRect.Top
                    ForegroundClientWidth = if ($foregroundGeometry) { $foregroundRect.Right - $foregroundRect.Left } else { $null }
                    ForegroundClientHeight = if ($foregroundGeometry) { $foregroundRect.Bottom - $foregroundRect.Top } else { $null }
                } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $outputRoot 'foreground-failure.json') -Encoding utf8
            }
            throw "The diagnostic window lost foreground ownership during sampling (expected=$window targetPID=$($live.Id) actual=$foreground foregroundPID=$foregroundOwner class=$foregroundClass)."
        }
        $sample = [PSCustomObject]@{
            Second = $second
            ElapsedSeconds = [math]::Round($wallMs / 1000, 3)
            SamplingOrigin = if ($RequireForeground) { 'ForegroundWindowReady' } else { 'ProcessLaunch' }
            WindowIdentitySource = if ($usesProbeReady) { 'ProbeReadyMarker' } elseif ($RequireForeground) { 'LargestVisibleWindow' } else { 'ProcessMainWindow' }
            SampledWindowHandle = $window.ToInt64()
            ActivationSeconds = $activationSeconds
            ProcessElapsedSeconds = [math]::Round($processWall.Elapsed.TotalSeconds, 3)
            PID = $live.Id
            PrivateMB = [math]::Round($live.PrivateMemorySize64 / 1MB, 1)
            WorkingSetMB = [math]::Round($live.WorkingSet64 / 1MB, 1)
            TaskManagerMB = $taskManagerMB
            Threads = $live.Threads.Count
            ClientWidthPx = if ($hasGeometry) { $clientRect.Right - $clientRect.Left } else { $null }
            ClientHeightPx = if ($hasGeometry) { $clientRect.Bottom - $clientRect.Top } else { $null }
            WindowDpi = if ($hasGeometry) { [TideTerm.StartupGeometry]::GetDpiForWindow($window) } else { $null }
            Minimized = if ($hasGeometry) { [TideTerm.StartupGeometry]::IsIconic($window) } else { $null }
            CPUOneCorePercent = [math]::Round(100 * ($cpuMs - $previousCpuMs) / ($wallMs - $previousWallMs), 3)
            CpuMilliseconds = [math]::Round($cpuMs, 3)
        }
        if ($ThreadCpu) {
            $sampleOwner = [uint32]0
            $guiThread = [TideTerm.StartupThread]::GetWindowThreadProcessId($window, [ref]$sampleOwner)
            if ($sampleOwner -ne $live.Id -or -not $guiThread) { throw 'Cannot identify the sampled GUI thread.' }
            $threadTimes = @(foreach ($thread in $live.Threads) {
                try {
                    [PSCustomObject]@{ Id = $thread.Id; CpuMilliseconds = [math]::Round($thread.TotalProcessorTime.TotalMilliseconds, 3) }
                } catch { if ($thread.Id -eq $guiThread) { throw } }
            })
            $sample | Add-Member -NotePropertyName GuiThreadId -NotePropertyValue $guiThread
            $sample | Add-Member -NotePropertyName ThreadCpu -NotePropertyValue $threadTimes
        }
        if ($RequireForeground) { $sample | Add-Member -NotePropertyName Foreground -NotePropertyValue $true }
        $sample
        $previousCpuMs = $cpuMs
        $previousWallMs = $wallMs
    }
    if ($ArgumentList -like '*--memory-probe*') {
        $probeSeconds = 8
        $configuredSeconds = 0
        if ([int]::TryParse($env:TIDETERM_MEMORY_PROBE_SECONDS, [ref]$configuredSeconds)) {
            $probeSeconds = [math]::Clamp($configuredSeconds, 8, 120)
        }
        # The probe timer starts after GUI construction, which can include font discovery.
        $waitMs = [int][math]::Max(3000, ($probeSeconds + 15) * 1000 - $wall.Elapsed.TotalMilliseconds)
        if (-not $process.WaitForExit($waitMs)) { throw 'Memory probe did not retire within its bounded lifetime.' }
        if ($process.ExitCode -ne 0) { throw "Memory probe failed: exit=$($process.ExitCode)" }
    }
} finally {
    $env:TIDETERM_WINDOW_PERF_PARENT_PID = $oldForegroundParent
    if ($process) {
        $live = Get-Process -Id $process.Id -ErrorAction SilentlyContinue
        if ($live -and $ArgumentList -notlike '*--memory-probe*') {
            $null = $process.CloseMainWindow()
            $null = $process.WaitForExit(2000)
        }
        Get-Process -Id $process.Id -ErrorAction SilentlyContinue |
            Stop-Process -Force -ErrorAction SilentlyContinue
    }
    if ($isNativeQt) {
        $env:PATH = $oldPath
        $env:QT_QPA_PLATFORM = $oldPlatform
    }
}
