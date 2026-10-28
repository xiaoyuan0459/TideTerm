# Windows/MSVC gate. SkipBuild analyses a historical log; it never certifies a fresh build.
[CmdletBinding()]
param(
    [ValidateSet('debug', 'release')][string]$Configuration = 'debug',
    [ValidateRange(1, 256)][int]$BuildJobs = 1,
    [string]$LogPath,
    [switch]$SkipBuild,
    [switch]$Offline
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$manifest = Join-Path $repo 'native/Cargo.toml'
if (-not $env:QT_DIR -or -not (Test-Path -LiteralPath "$env:QT_DIR/include/QtCore")) {
    throw 'Set QT_DIR to a Qt MSVC SDK with include/QtCore.'
}
function Normalize-Path([string]$Path) {
    $p = $Path.Trim().Trim('"').Replace('\', '/')
    if (-not [IO.Path]::IsPathRooted($p)) { $p = Join-Path (Join-Path $repo 'native') $p }
    [IO.Path]::GetFullPath($p).Replace('\', '/').ToLowerInvariant()
}
$qtInclude = (Normalize-Path "$env:QT_DIR/include") + '/'
$repoPrefix = (Normalize-Path $repo) + '/'
if ($SkipBuild -and -not $LogPath) { throw '-SkipBuild requires -LogPath.' }
if (-not $LogPath) {
    $LogPath = Join-Path $repo "native/target/diagnostics/qt612-remediation/cpp-$Configuration-$(Get-Date -Format yyyyMMdd-HHmmss).log"
}
$LogPath = [IO.Path]::GetFullPath($LogPath)
$buildExit = 0
if (-not $SkipBuild) {
    # Reject inherited suppressions rather than silently weakening this gate.
    $flagVariables = Get-ChildItem Env: | Where-Object { $_.Name -match '^(?:(?:HOST_|TARGET_)?CXXFLAGS|CL$|_CL_$)' }
    foreach ($entry in $flagVariables) {
        if ($entry.Value -match '(?i)(/w(?:[0-3]|d\d+|o)?(?:\s|$)|-w(?:\s|$)|external:W|QT_(?:NO_DEPRECATED_WARNINGS|WARN_DEPRECATED_UP_TO|DISABLE_DEPRECATED_UP_TO))') {
            throw "Remove warning suppressions from $($entry.Name) before running the gate."
        }
    }
    $exe = Join-Path $repo "native/target/$Configuration/native.exe"
    if (Get-Process -Name native -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe }) {
        throw 'Close this build of native.exe before cleaning/relinking it.'
    }
    New-Item -ItemType Directory -Force -Path (Split-Path $LogPath) | Out-Null
    # Use the same prepared environment as ordinary builds. Native's cc::Build
    # emits compiler invocations itself, without changing dependency inputs.
    Push-Location $repo
    try {
        $cleanProfile = if ($Configuration -eq 'release') { 'release' } else { 'dev' }
        & cargo clean --manifest-path $manifest -p native --profile $cleanProfile
        if ($LASTEXITCODE -ne 0) { throw "cargo clean failed: $LASTEXITCODE" }
        # Concurrent cc-rs output can splice compiler paths and diagnostics.
        # Serialize the gate's evidence stream; ordinary builds choose their own jobs.
        $buildArgs = @('build', '--manifest-path', $manifest, '--bin', 'native', '--locked', '--jobs', "$BuildJobs", '--message-format=json', '-vv')
        if ($Configuration -eq 'release') { $buildArgs += '--release' }
        if ($Offline) { $buildArgs += '--offline' }
        # Native stderr is data, not a PowerShell terminating error.
        $PSNativeCommandUseErrorActionPreference = $false
        & cargo @buildArgs 2>&1 | Tee-Object -FilePath $LogPath | Out-Null
        $buildExit = $LASTEXITCODE
        Add-Content -LiteralPath $LogPath -Value "CPP_GATE_CARGO_EXIT=$buildExit"
    } finally { Pop-Location }
}
if (-not (Test-Path -LiteralPath $LogPath)) { throw "Missing log: $LogPath" }
$lines = Get-Content -LiteralPath $LogPath
$own = 0; $known = 0; $other = 0; $errors = 0; $rustCargo = 0
foreach ($line in $lines) {
    # cc::Build::expand forwards MSVC's probe source banner as cargo:warning.
    # This exact basename is compiler chatter; coded diagnostics and any suffix
    # still go through the warning/error classifier below.
    if ($line -match '^(?:\[openssl-sys [0-9.]+\] cargo:warning=|warning: openssl-sys@[0-9.]+: )expando\.c$') { continue }
    # cc-rs probes cl.exe and forwards its localized copyright banner. On CP936
    # hosts the non-ASCII prefix decodes as mojibake, so match the stable ASCII
    # product name rather than the localized "Copyright (C)" wording.
    if ($line -match '^(?:\[[^\]]+\]\s*)?cargo:warning=.*Microsoft Corporation') { continue }
    # Cargo's JSON channel keeps multi-line Rust diagnostics out of the C++
    # classifier. Build-script/MSVC output remains plain text below.
    if ($line.StartsWith('{')) {
        $message = $null
        try { $message = $line | ConvertFrom-Json -ErrorAction Stop } catch { }
        if ($message.reason -eq 'compiler-message') {
            if ($message.message.level -eq 'warning') {
                $linkWarnings = if ($message.message.code.code -eq 'linker_messages') {
                    @([regex]::Matches([string]$message.message.message, '(?i)\bwarning\s+(LNK\d+)\s*[:：]'))
                } else { @() }
                if ($linkWarnings.Count) {
                    $other += $linkWarnings.Count
                    $codes = ($linkWarnings | ForEach-Object { $_.Groups[1].Value.ToUpperInvariant() } | Sort-Object -Unique) -join ', '
                    Write-Output "OTHER (Rust linker): $codes ($($linkWarnings.Count) diagnostics)"
                } else { $rustCargo++ }
            }
            if ($message.message.level -eq 'error') { $errors++ }
            continue
        }
        if ($message.reason -in @('compiler-artifact', 'build-script-executed', 'build-finished')) { continue }
    }
    $diagnostic = $line -replace '^\[[^\]]+\]\s*', '' -replace '^cargo:warning=', '' -replace '^warning: [^:]+@[^:]+:\s*', ''
    # Includes Cargo prefixes, command-line D warnings and linker warnings.
    if ($line -match '(?i)\bwarning\s+([CD]\d{4}|LNK\d+)\s*[:：]') {
        $code = $Matches[1].ToUpperInvariant()
        $path = $null
        if ($diagnostic -match '^(.*?)\(\d+(?:,\d+)?\)\s*:\s*warning') {
            $path = Normalize-Path $Matches[1]
        }
        if ($path -and $code -eq 'C4996' -and $line -match "'Q(?:Core|Gui)Application::compressEvent'" -and
            $path -in @("${qtInclude}qtgui/qguiapplication.h", "${qtInclude}qtwidgets/qapplication.h")) {
            $known++
        } elseif ($path -and $path.StartsWith($repoPrefix)) {
            $own++; Write-Output "PROJECT: $line"
        } else { $other++; Write-Output "OTHER: $line" }
    } elseif ($diagnostic -match '^Microsoft \(R\) C/C\+\+ Optimizing Compiler Version |^(?:版权所有\(C\)|Copyright \(C\)) Microsoft Corporation|^\s*$' -or
        $line -match '^(?:\[zmodem2 [^\]]+\] cargo:warning=|warning: zmodem2@[^:]+: )lrzsz not found$' -or
        $line -match "^warning: [^:]+@[^:]+: ignoring 'hints\.[^']+', pass .*-Zprofile-hint-") {
        # cc-rs compiler-version probe banners and an optional external tool notice.
        # These are not compiler diagnostics; retain them in the complete log.
        if ($line -match '\bwarning\b') { $rustCargo++ }
    } elseif ($line -match '^warning: ' -and $line -notmatch '^warning: [^:]+@[^:]+:') {
        $rustCargo++ # Rust/Cargo diagnostics belong to their own gates.
    } elseif ($line -match '(?i)\bwarning\b' -and $line -notmatch 'running:|warnings_into_errors') {
        # Unknown compiler diagnostic format must not disappear from the verdict.
        $other++; Write-Output "UNCLASSIFIED: $line"
    }
    if ($line -match '(?i)(?:fatal\s+)?error\s+(?:C\d{4}|D\d{4}|LNK\d+)\s*[:：]|^error:|CPP_GATE_CARGO_EXIT=[1-9]') { $errors++ }
}
$raw = $lines -join "`n"
$finished = $raw -match 'Finished `(?:dev|release)` profile'
# cc-rs debug output shows a compiler executing /c, not just environment probes.
$invocations = @($lines | Where-Object { $_ -match '\[native [^\]]+\] running: .*[/\\]cl\.exe".*"[-/]c"\s+"(?:[^"\r\n]*[/\\]cpp[/\\]|cpp[/\\])[^"\r\n]*\.cpp"' })
$compiled = $invocations.Count -gt 0 -and $raw -match 'native_cpp'
Write-Output "C++ diagnostics: project=$own known-Qt=$known other=$other errors=$errors; separate Rust/Cargo warnings=$rustCargo"
Write-Output "Log: $LogPath"
if ($SkipBuild) { Write-Output 'Historical analysis only; no fresh-build certification.' }
else {
    Write-Output "Fresh compile evidence=$compiled completed=$finished Cargo exit=$buildExit Qt=$env:QT_DIR profile=$Configuration /W4 default Qt deprecation range"
    $invocation = $invocations | Select-Object -First 1
    if ($invocation) { Write-Output $invocation }
}
if ($own -or $other -or $errors -or $buildExit -ne 0 -or -not $finished -or -not $compiled) { exit 1 }
exit 0
