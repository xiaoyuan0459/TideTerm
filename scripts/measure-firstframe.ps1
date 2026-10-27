#requires -Version 7
# 首屏耗时 / 热启动 / CPU 采样探针。measure-startup.ps1 只给内存与线程，
# 本脚本补上「进程起来 → 主窗口可交互」这一段，并给出后台恢复的耗时。
# 用法:
#   pwsh scripts/measure-firstframe.ps1 -ExePath native/target/release/native.exe -Rounds 5
#   pwsh scripts/measure-firstframe.ps1 -ExePath <exe> -ArgumentList '--window' -SampleSeconds 20
# 冷启动口径：进程创建 → 主窗口句柄出现（MainWindowHandle != 0）。
# 热启动口径：最小化后恢复到前台，取窗口重绘完成前的首个消息泵空闲点。
param(
    [Parameter(Mandatory = $true)][string]$ExePath,
    [string]$ArgumentList = '--window',
    [int]$Rounds = 3,
    [int]$SampleSeconds = 10,
    [int]$IntervalMs = 20
)

$resolved = (Resolve-Path -LiteralPath $ExePath -ErrorAction Stop).Path
$exeName = [IO.Path]::GetFileNameWithoutExtension($resolved)

# Qt 运行目录要在 PATH 里，否则进程静默起不来
if ($env:QT_DIR) {
    $qtBin = Join-Path $env:QT_DIR 'bin'
    if (-not (Test-Path -LiteralPath $qtBin)) { throw "QT_DIR/bin 不存在：$qtBin" }
    $env:PATH = "$qtBin$([IO.Path]::PathSeparator)$env:PATH"
}

$warmup = Start-Process -FilePath $resolved -ArgumentList $ArgumentList -PassThru
Start-Sleep -Seconds 3
Stop-Process -Id $warmup.Id -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 500

$cold = @()
for ($i = 1; $i -le $Rounds; $i++) {
    $p = Start-Process -FilePath $resolved -ArgumentList $ArgumentList -PassThru
    $t0 = Get-Date
    $handleAt = $null
    while (((Get-Date) - $t0).TotalMilliseconds -lt 15000) {
        $p = Get-Process -Id $p.Id -ErrorAction SilentlyContinue
        if (-not $p) { break }
        if ($p.MainWindowHandle -ne 0) { $handleAt = Get-Date; break }
        Start-Sleep -Milliseconds $IntervalMs
    }
    if ($handleAt) {
        $ms = [math]::Round(($handleAt - $t0).TotalMilliseconds, 1)
        $cold += [pscustomobject]@{ Round = $i; ColdStartMs = $ms; WS_MB = [math]::Round($p.WorkingSet64 / 1MB, 1) }
    } else {
        $cold += [pscustomobject]@{ Round = $i; ColdStartMs = -1; WS_MB = 0 }
    }
    if ($p) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
    Start-Sleep -Milliseconds 500
}

# 热启动：常驻进程最小化 -> 还原，测往返耗时
$hot = $null
$pv = Start-Process -FilePath $resolved -ArgumentList $ArgumentList -PassThru
Start-Sleep -Seconds 4
$pv = Get-Process -Id $pv.Id -ErrorAction SilentlyContinue
if ($pv) {
    Add-Type @"
using System;using System.Runtime.InteropServices;
public static class Win{
 [DllImport("user32.dll")]public static extern bool ShowWindow(IntPtr h,int c);
 [DllImport("user32.dll")]public static extern bool SetForegroundWindow(IntPtr h);
}
"@
    $h = $pv.MainWindowHandle
    $samples = @()
    for ($i = 1; $i -le 3; $i++) {
        [Win]::ShowWindow($h, 6) | Out-Null   # SW_MINIMIZE
        Start-Sleep -Milliseconds 400
        $t0 = Get-Date
        [Win]::ShowWindow($h, 9) | Out-Null   # SW_RESTORE
        [Win]::SetForegroundWindow($h) | Out-Null
        Start-Sleep -Milliseconds 150
        $samples += [math]::Round(((Get-Date) - $t0).TotalMilliseconds, 1)
    }
    $hot = ($samples | Measure-Object -Average -Maximum)
    "hot_restore_samples_ms: $($samples -join ', ')"
}

# 稳态 CPU / 内存采样（空闲 + 无网络会话）
if ($pv -and -not $pv.HasExited) {
    $start = $pv.CPU; $t0 = Get-Date; $peakWS = 0; $threads = 0
    while (((Get-Date) - $t0).TotalSeconds -lt $SampleSeconds) {
        $pv = Get-Process -Id $pv.Id -ErrorAction SilentlyContinue
        if (-not $pv) { break }
        $peakWS = [math]::Max($peakWS, $pv.WorkingSet64 / 1MB)
        $threads = $pv.Threads.Count
        Start-Sleep -Milliseconds 200
    }
    $cpuMs = ((Get-Process -Id $pv.Id -ErrorAction SilentlyContinue).CPU - $start) * 1000
    $wall = ((Get-Date) - $t0).TotalMilliseconds
    $cores = [Environment]::ProcessorCount
    "idle_cpu_percent_of_one_core: $([math]::Round($cpuMs / $wall * 100, 2))"
    "idle_cpu_percent_of_machine : $([math]::Round($cpuMs / $wall / $cores * 100, 2))  (cores=$cores)"
    "steady_peak_ws_mb: $([math]::Round($peakWS, 1))  threads: $threads"
    Stop-Process -Id $pv.Id -Force -ErrorAction SilentlyContinue
}

"--- cold start (round, ms, WS_MB) ---"
$cold | ForEach-Object { "{0}  {1}  {2}" -f $_.Round, $_.ColdStartMs, $_.WS_MB }
$ok = $cold | Where-Object ColdStartMs -ge 0 | ForEach-Object ColdStartMs
if ($ok) {
    $s = $ok | Sort-Object
    "COLD_MS median={0} min={1} max={2} over {3} rounds (首屏判据: 窗口句柄出现)" -f $s[[int][math]::Floor($s.Count / 2)], $s[0], $s[-1], $s.Count
}
if ($hot) { "HOT_RESTORE_MS avg={0} max={1}" -f [math]::Round($hot.Average, 1), [math]::Round($hot.Maximum, 1) }
