#requires -Version 7
# 内存探针（--memory-probe 各档）的**进程级**资源采样器。
# C++ 侧 qt_memory_probe 只报「窗格数 / 累计绘制 / 每格文本」，不报进程 PrivateMB/WS/CPU；
# 维度 6 的「内存峰值 ≤200 MB」「CPU 单场景 ≤20% / 持续 ≤75%」「并发（8 格）资源占用」
# 需要把这一层补上，否则每格驻留只能靠 docs 里的历史数字说话。
#
# 用法（先在 native 下 cargo build --release）:
#   pwsh scripts/measure-probe-memory.ps1 -Stage pane8-grid
#   pwsh scripts/measure-probe-memory.ps1 -Stage solo-cjk -Rounds 3
#   pwsh scripts/measure-probe-memory.ps1 -Stage full -IntervalMs 100 -TimeoutSeconds 60
#
# 口径（与 measure-firstframe.ps1 对齐，避免两套口径互相打架）:
#   · PrivateMB  峰值 = PrivateMemorySize64 的 max（私有提交，含未驻留部分）
#   · WorkingSetMB 峰值 = WorkingSet64 的 max（物理驻留）
#   · CPU%        「单核」= 采样窗口内 TotalProcessorTime 增量 / 墙钟增量，
#                 「整机」= 同一增量再除以 [Environment]::ProcessorCount
#                 （32 核机上空闲 1.95% 单核 = 0.06% 整机，两套都必须写，否则数字会被读错）
#   · 墙钟        Start-Process → 进程退出
param(
    [Parameter(Mandatory = $true)][string]$Stage,
    [string]$ExePath = 'native/target/release/native.exe',
    [int]$Rounds = 3,
    [int]$IntervalMs = 200,
    [int]$TimeoutSeconds = 90,
    # 原始采样落盘位置；默认丢临时目录，别往仓库根里丢 CSV
    [string]$CsvPath = ''
)

$ErrorActionPreference = 'Stop'
$resolved = (Resolve-Path -LiteralPath $ExePath -ErrorAction Stop).Path

# Qt 运行目录要在 PATH 里，否则进程静默起不来（只会报 loading shared libraries 失败）
if ($env:QT_DIR) {
    $qtBin = Join-Path $env:QT_DIR 'bin'
    if (-not (Test-Path -LiteralPath $qtBin)) { throw "QT_DIR/bin 不存在：$qtBin" }
    $env:PATH = "$qtBin$([IO.Path]::PathSeparator)$env:PATH"
}
$env:QT_QPA_PLATFORM = 'offscreen'

$cores = [Environment]::ProcessorCount
$rows = @()
for ($round = 1; $round -le $Rounds; $round++) {
    $log = [IO.Path]::GetTempFileName()
    $p = Start-Process -FilePath $resolved -ArgumentList "--memory-probe --$Stage" `
        -PassThru -RedirectStandardOutput $log
    $peakPriv = 0L; $peakWs = 0L; $peakThreads = 0
    $samples = 0
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while (-not $p.HasExited -and (Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds $IntervalMs
        if ($p.HasExited) { break }
        try { $p.Refresh() } catch { break }
        $peakPriv = [Math]::Max($peakPriv, $p.PrivateMemorySize64)
        $peakWs = [Math]::Max($peakWs, $p.WorkingSet64)
        $peakThreads = [Math]::Max($peakThreads, $p.Threads.Count)
        $samples++
    }
    $p.WaitForExit()
    $cpuTotalMs = $p.TotalProcessorTime.TotalMilliseconds
    $wallS = [math]::Max(0.001, $samples * $IntervalMs / 1000.0)
    # 单核占用 = CPU 时间 / 墙钟（%）；整机 = 再除以核数
    $singleCore = [math]::Min(100.0, $cpuTotalMs / ($wallS * 10))
    $probe = (Get-Content -LiteralPath $log -Raw -ErrorAction SilentlyContinue)
    Remove-Item -LiteralPath $log -Force -ErrorAction SilentlyContinue
    $rows += [pscustomobject]@{
        Stage          = $Stage
        PrivateMB      = [math]::Round($peakPriv / 1MB, 1)
        WorkingSetMB   = [math]::Round($peakWs / 1MB, 1)
        Threads        = $peakThreads
        CpuMs          = [math]::Round($cpuTotalMs, 1)
        WallS          = [math]::Round($wallS, 2)
        SingleCorePct  = [math]::Round($singleCore, 2)
        AllCoresPct    = [math]::Round($singleCore / $cores, 3)
    }
    Write-Host "[round $round] Private=$($rows[-1].PrivateMB)MB WS=$($rows[-1].WorkingSetMB)MB threads=$peakThreads cpuMs=$($rows[-1].CpuMs) wall=$($rows[-1].WallS)s single=$($rows[-1].SingleCorePct)% allcores=$($rows[-1].AllCoresPct)% ($cores cores)"
    if ($probe) { Write-Host "  probe: $((($probe -split "`n") | Where-Object { $_ -match 'probe' } | Select-Object -Last 1).Trim())" }
}

if ([string]::IsNullOrWhiteSpace($CsvPath)) { $CsvPath = Join-Path $env:TEMP "probe-$Stage.csv" }
$rows | Export-Csv -NoTypeInformation -Path $CsvPath -Encoding utf8
Write-Host "  原始采样: $CsvPath"
$avg = $rows | Measure-Object -Property PrivateMB, WorkingSetMB, SingleCorePct -Average
Write-Host ""
Write-Host "── $Stage：$Rounds 轮均值 ──"
Write-Host ("  PrivateMB  avg={0} max={1}   WorkingSetMB avg={2} max={3}   CPU单核 avg={4}% max={5}%  整机 avg={6}%" -f `
    [math]::Round($avg[0].Average, 1), ($rows | Measure-Object -Property PrivateMB -Maximum).Maximum, `
    [math]::Round($avg[1].Average, 1), ($rows | Measure-Object -Property WorkingSetMB -Maximum).Maximum, `
    [math]::Round($avg[2].Average, 2), ($rows | Measure-Object -Property SingleCorePct -Maximum).Maximum, `
    [math]::Round((($rows | Measure-Object -Property AllCoresPct -Average).Average), 3))
