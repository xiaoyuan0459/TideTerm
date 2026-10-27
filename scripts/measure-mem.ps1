# 内存测量 v2：启动 TideTerm，递归收集主进程 + 所有后代进程（含 WebView2）内存
# 用法: pwsh measure-mem.ps1 -ExePath <exe> [-WaitSec N] [-CooldownMs N] [-Keep]
param(
    [Parameter(Mandatory=$true)][string]$ExePath,
    [int]$WaitSec = 10,
    [int]$CooldownMs = 3000,
    [switch]$Keep
)

function Get-DescendantPids {
    param([int]$RootPid)
    $all = Get-CimInstance Win32_Process -ErrorAction SilentlyContinue
    $desc = New-Object System.Collections.Generic.List[int]
    $frontier = @($RootPid)
    $allByParent = @{}
    foreach ($p in $all) {
        if ($allByParent.ContainsKey([int]$p.ParentProcessId)) {
            $allByParent[[int]$p.ParentProcessId] += [int]$p.ProcessId
        } else {
            $allByParent[[int]$p.ParentProcessId] = @([int]$p.ProcessId)
        }
    }
    while ($frontier.Count -gt 0) {
        $next = @()
        foreach ($pid0 in $frontier) {
            if ($allByParent.ContainsKey($pid0)) {
                foreach ($c in $allByParent[$pid0]) {
                    if (-not $desc.Contains($c)) { $desc.Add($c); $next += $c }
                }
            }
        }
        $frontier = $next
    }
    return $desc
}

$proc = Start-Process -FilePath $ExePath -PassThru
$rootPid = $proc.Id
Write-Output "启动主进程 PID=$rootPid，等待 $WaitSec 秒稳定..."
Start-Sleep -Seconds $WaitSec

$scriptPid = $PID

function Sample {
    $pids = @($rootPid) + @(Get-DescendantPids $rootPid)
    $rows = @()
    $total = 0
    foreach ($id in $pids) {
        if ($id -eq $scriptPid) { continue }
        $p = Get-Process -Id $id -ErrorAction SilentlyContinue
        if ($p) {
            $total += $p.WorkingSet64
            $rows += [PSCustomObject]@{
                PID = $p.Id
                Name = $p.ProcessName
                WorkingSetMB = [math]::Round($p.WorkingSet64/1MB,1)
                PrivateMB = [math]::Round($p.PrivateMemorySize64/1MB,1)
            }
        }
    }
    return [PSCustomObject]@{ Rows=$rows; Total=$total; Count=$rows.Count }
}

$s1 = Sample
Start-Sleep -Milliseconds $CooldownMs
$s2 = Sample

Write-Output "`n=== 进程明细 (第二次采样) ==="
$s2.Rows | Sort-Object WorkingSetMB -Descending | Format-Table -AutoSize

Write-Output "=== 汇总 (主进程 + $($s2.Count) 个后代) ==="
Write-Output ("第一次采样 总工作集: {0} MB" -f [math]::Round($s1.Total/1MB,1))
Write-Output ("第二次采样 总工作集: {0} MB" -f [math]::Round($s2.Total/1MB,1))
Write-Output ("进程数: {0}" -f $s2.Count)

if (-not $Keep) {
    Get-Process -Id $rootPid -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    foreach ($id in @(Get-DescendantPids $rootPid)) {
        Get-Process -Id $id -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    }
    Write-Output "已关闭应用及全部后代进程"
}