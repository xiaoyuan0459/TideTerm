#Requires -Version 7.0
param(
    [switch]$Offline,
    [ValidateSet('production', 'session')][string]$Scope = 'production'
)

$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
Push-Location $repoRoot
try {
    # Keep the pinned audit tool and reports in the ignored build directory.
    $toolCache = Join-Path $repoRoot 'native/target/tool-cache'
    $arguments = @('--yes', '--cache', $toolCache, '--registry=https://registry.npmjs.org')
    if ($Offline) { $arguments += '--offline' }
    $paths = @('native/cpp', 'native/src', 'crates/tiderm-core/src')
    if ($Scope -eq 'session') {
        $paths = @(
            'native/cpp/chrome/session_form.cpp', 'native/cpp/chrome/session_form.h',
            'native/cpp/chrome/session_form_fields.cpp', 'native/cpp/chrome/session_form_settings.cpp',
            'native/cpp/chrome/session_form_advanced.cpp', 'native/cpp/chrome/session_form_values.cpp',
            'native/cpp/chrome/session_form_widgets.h', 'native/src/rust/session_form_config.rs',
            'native/src/rust/session_advanced.rs', 'native/src/rust/session_defaults.rs',
            'crates/tiderm-core/src/session_options.rs'
        )
    }
    $arguments += 'jscpd@4.0.5'
    $arguments += $paths
    $arguments += @(
        '--format', 'cpp,rust',
        '--formats-exts', 'cpp:cpp,h,hpp,cc,cxx;rust:rs',
        '--mode', 'mild', '--min-lines', '5', '--min-tokens', '50',
        '--ignore', '**/*selftest*,**/*tests*,**/test*,**/shared/theme_db/**',
        '--reporters', 'json,console',
        '--output', "native/target/duplication-$Scope",
        '--threshold', '4.99'
    )
    & npx @arguments
    if ($LASTEXITCODE -ne 0) { throw "Duplication audit failed (exit $LASTEXITCODE)." }
} finally {
    Pop-Location
}
