# measure-peak.ps1: launch an exe, sample its working set every 200ms until it exits,
# print a time series so stage peaks (fontdue vs ab_glyph) are visible.
param(
    [Parameter(Mandatory=$true)][string]$ExePath,
    [int]$IntervalMs = 200,
    [int]$MaxSecs = 40
)
$proc = Start-Process -FilePath $ExePath -PassThru -NoNewWindow
$samples = @()
$deadline = (Get-Date).AddSeconds($MaxSecs)
while (-not $proc.HasExited -and (Get-Date) -lt $deadline) {
    $p = Get-Process -Id $proc.Id -ErrorAction SilentlyContinue
    if ($p) {
        $samples += [PSCustomObject]@{ T=[math]::Round((New-TimeSpan -Start $proc.StartTime).TotalSeconds,1); WSMB=[math]::Round($p.WorkingSet64/1MB,1) }
    }
    Start-Sleep -Milliseconds $IntervalMs
}
Write-Output "=== time series (t_sec, WS_MB) ==="
$samples | ForEach-Object { "{0,5} {1,8}" -f $_.T, $_.WSMB }
if ($samples.Count -gt 0) {
    $peak = $samples | Sort-Object WSMB -Descending | Select-Object -First 1
    Write-Output ("PEAK_WS_MB={0} at t={1}s" -f $peak.WSMB, $peak.T)
}
