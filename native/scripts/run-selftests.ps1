# Qt 壳自检批跑：offscreen 可复现，全部 exit 0 才算过。
# 用法：pwsh native/scripts/run-selftests.ps1 [-Configuration release]
param(
    [ValidateSet('debug', 'release')][string]$Configuration = 'debug',
    [string]$LogDirectory
)
$ErrorActionPreference = 'Continue'

$root = Split-Path -Parent $PSScriptRoot
if ($env:QT_DIR) {
    $qtBin = Join-Path $env:QT_DIR 'bin'
    if (-not (Test-Path $qtBin)) { throw "QT_DIR/bin 不存在：$qtBin" }
    $env:PATH = "$qtBin$([IO.Path]::PathSeparator)$env:PATH"
}
$env:QT_QPA_PLATFORM = 'offscreen'
$exe = Join-Path $root "target/$Configuration/native.exe"
if (-not (Test-Path $exe)) { Write-Error "找不到 $exe，先 cargo build"; exit 1 }
$logDir = if ($LogDirectory) { [IO.Path]::GetFullPath($LogDirectory) } else { Join-Path $root 'target/selftests' }
New-Item -ItemType Directory -Force -Path $logDir | Out-Null

# 全量子检开关（与 native/src/main.rs 分发一一对应；漏一个就少一层证据）。
$flags = @(Get-Content -LiteralPath (Join-Path $PSScriptRoot 'selftest-flags.txt') | Where-Object { $_ })

$failed = 0
foreach ($f in $flags) {
    $out = Join-Path $logDir ("self-$f.log")
    # Direct invocation preserves the configured Qt DLL PATH under redirected output.
    & $exe $f 1> $out 2> "$out.err"
    $exitCode = $LASTEXITCODE
    $tail = (Get-Content $out -Encoding UTF8 | Select-Object -Last 2) -join ' | '
    if ($exitCode -ne 0) { $failed++ }
    Write-Output ("{0,-22} exit={1}  {2}" -f $f, $exitCode, $tail)
}
$groupShot = Join-Path $root 'target/groups-independent.png'
if (Test-Path -LiteralPath $groupShot) {
    Move-Item -LiteralPath $groupShot -Destination (Join-Path $logDir 'groups-independent.png') -Force
}
if ($failed -gt 0) { Write-Error "$failed 个自检失败"; exit 1 }
Write-Output "全部 $($flags.Count) 个自检通过"
