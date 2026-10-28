# Dot-source before Windows builds. Changes apply only to this build process.
$ErrorActionPreference = 'Stop'
if (-not $IsWindows) { throw 'This helper is for Windows MSVC builds' }
$env:LANG = 'C'
$env:LC_ALL = 'C'
$env:LC_CTYPE = 'C'
$perlCommand = Get-Command perl -ErrorAction Stop
& $perlCommand.Source -MIPC::Cmd -MFindBin -e 'exit 0'
if ($LASTEXITCODE -ne 0) { throw 'Perl core modules for OpenSSL are missing' }
$buildTools = Join-Path (Split-Path -Parent $PSScriptRoot) 'target/build-tools'
New-Item -ItemType Directory -Force -Path $buildTools | Out-Null
$adapterSource = Join-Path $PSScriptRoot 'openssl-perl.rs'
$adapterVersion = (Get-FileHash -LiteralPath $adapterSource -Algorithm SHA256).Hash.Substring(0, 16).ToLowerInvariant()
$adapter = Join-Path $buildTools "openssl-perl-$adapterVersion.exe"
if (-not (Test-Path -LiteralPath $adapter)) {
    & rustc --edition=2024 -O $adapterSource -o $adapter
    if ($LASTEXITCODE -ne 0) { throw 'OpenSSL build adapter compilation failed' }
}
# openssl-src does not track this adapter as a Cargo input. Keep its migration
# marker beside cached dependency outputs, so restored CI caches retain it too.
$nativeTarget = Split-Path -Parent $buildTools
$adapterName = Split-Path -Leaf $adapter
$opensslOutputs = @(Get-ChildItem -Path (Join-Path $nativeTarget '*/build/openssl-sys-*/output') -File -ErrorAction SilentlyContinue)
$staleOutputs = @($opensslOutputs | Where-Object {
    $marker = Join-Path $_.DirectoryName 'out/tideterm-adapter-version'
    -not (Test-Path -LiteralPath $marker) -or (Get-Content -LiteralPath $marker -Raw).Trim() -ne $adapterName
})
foreach ($staleProfileDirectory in @($staleOutputs | ForEach-Object { $_.Directory.Parent.Parent.Name } | Sort-Object -Unique)) {
    $cacheProfile = if ($staleProfileDirectory -eq 'debug') { 'dev' } else { $staleProfileDirectory }
    & cargo clean --manifest-path (Join-Path (Split-Path -Parent $PSScriptRoot) 'Cargo.toml') -p openssl-sys --profile $cacheProfile --locked --offline
    if ($LASTEXITCODE -ne 0) { throw 'OpenSSL adapter cache migration failed' }
}
$env:TIDETERM_BUILD_PERL = $perlCommand.Source
$env:OPENSSL_SRC_PERL = $adapter
if ($env:_CL_ -notmatch '(?:^|\s)/utf-8(?:\s|$)') { $env:_CL_ = "$env:_CL_ /utf-8" }
# CXX's runtime needs exception unwinding too. Keep normal builds and gates identical.
if ($env:CXXFLAGS -notmatch '(?:^|\s)/EHsc(?:\s|$)') { $env:CXXFLAGS = "$env:CXXFLAGS /EHsc" }
