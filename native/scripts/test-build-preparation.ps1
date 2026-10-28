# Exercise cache migration with real Cargo artifacts in an isolated workspace.
[CmdletBinding()]
param([string]$PreparationScript = (Join-Path $PSScriptRoot 'prepare-windows-build.ps1'))
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
if (-not $IsWindows) { throw 'Windows build preparation regression' }
$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$fixture = Join-Path $repo ".tmp/build-preparation-$([guid]::NewGuid().ToString('N'))/native"
$fixtureScripts = Join-Path $fixture 'scripts'
foreach ($part in @('scripts','src','openssl/src','bystander/src')) {
    New-Item -ItemType Directory -Force -Path (Join-Path $fixture $part) | Out-Null
}
Copy-Item -LiteralPath $PreparationScript -Destination (Join-Path $fixtureScripts 'prepare-windows-build.ps1')
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'openssl-perl.rs') -Destination $fixtureScripts
@'
[workspace]
[package]
name = "build-preparation-fixture"
version = "0.0.0"
edition = "2024"
[dependencies]
openssl-sys = { path = "openssl" }
build-bystander = { path = "bystander" }
'@ | Set-Content (Join-Path $fixture 'Cargo.toml')
'fn main() { openssl_sys::fixture(); build_bystander::fixture(); }' | Set-Content (Join-Path $fixture 'src/main.rs')
foreach ($package in @(@('openssl','openssl-sys'), @('bystander','build-bystander'))) {
    @"
[package]
name = "$($package[1])"
version = "0.0.0"
edition = "2024"
"@ | Set-Content (Join-Path $fixture "$($package[0])/Cargo.toml")
    'pub fn fixture() {}' | Set-Content (Join-Path $fixture "$($package[0])/src/lib.rs")
}
@'
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if let Some(adapter) = std::env::var_os("OPENSSL_SRC_PERL") {
        let adapter = std::path::Path::new(&adapter);
        let marker = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap())
            .join("tideterm-adapter-version");
        std::fs::write(marker, adapter.file_name().unwrap().to_string_lossy().as_bytes()).unwrap();
    }
}
'@ | Set-Content (Join-Path $fixture 'openssl/build.rs')
$fixtureManifest = Join-Path $fixture 'Cargo.toml'
$savedTarget = $env:CARGO_TARGET_DIR
$savedPerl = $env:OPENSSL_SRC_PERL
try {
    $env:CARGO_TARGET_DIR = Join-Path $fixture 'target'
    Remove-Item Env:OPENSSL_SRC_PERL -ErrorAction SilentlyContinue
    & cargo generate-lockfile --manifest-path $fixtureManifest --offline
    if ($LASTEXITCODE -ne 0) { throw 'Fixture lockfile failed' }
    & cargo build --manifest-path $fixtureManifest --locked --offline
    if ($LASTEXITCODE -ne 0) { throw 'Fixture initial build failed' }
    & cargo build --manifest-path $fixtureManifest --release --locked --offline
    if ($LASTEXITCODE -ne 0) { throw 'Fixture Release build failed' }
    $staleArtifacts = @(Get-ChildItem -Path "$fixture/target/*/deps/*openssl_sys*.rlib" -File)
    if ($staleArtifacts.Count -ne 2) { throw 'Fixture did not produce both profiles' }
    $bystanders = @(Get-ChildItem -Path "$fixture/target/*/deps/*build_bystander*.rlib" -File |
        ForEach-Object { [pscustomobject]@{ path=$_.FullName; hash=(Get-FileHash -LiteralPath $_.FullName).Hash; time=$_.LastWriteTimeUtc.Ticks } })
    if ($bystanders.Count -ne 2) { throw 'Fixture did not produce unrelated artifacts' }
    . (Join-Path $fixtureScripts 'prepare-windows-build.ps1')
    foreach ($artifact in $staleArtifacts) {
        if (Test-Path -LiteralPath $artifact.FullName) { throw "Stale OpenSSL artifacts survived preparation: $($artifact.FullName)" }
    }
    foreach ($file in $bystanders) {
        if ((Get-FileHash -LiteralPath $file.path).Hash -ne $file.hash -or
            (Get-Item $file.path).LastWriteTimeUtc.Ticks -ne $file.time) { throw 'Unrelated Cargo artifact changed' }
    }
    & cargo build --manifest-path $fixtureManifest --locked --offline
    if ($LASTEXITCODE -ne 0) { throw 'Fixture rebuild failed' }
    $cache = @(Get-ChildItem -Path "$fixture/target/debug/.fingerprint/openssl-sys-*" -Directory |
        ForEach-Object { Get-ChildItem -LiteralPath $_.FullName -File } |
        ForEach-Object { [pscustomobject]@{ path=$_.FullName; hash=(Get-FileHash -LiteralPath $_.FullName).Hash; time=$_.LastWriteTimeUtc.Ticks } })
    if (-not $cache.Count) { throw 'Fixture has no dependency fingerprints' }
    $adapterTime = (Get-Item $env:OPENSSL_SRC_PERL).LastWriteTimeUtc.Ticks
    . (Join-Path $fixtureScripts 'prepare-windows-build.ps1')
    . (Join-Path $fixtureScripts 'prepare-windows-build.ps1')
    foreach ($file in $cache) {
        if ((Get-FileHash -LiteralPath $file.path).Hash -ne $file.hash -or
            (Get-Item $file.path).LastWriteTimeUtc.Ticks -ne $file.time) { throw 'Matching dependency cache was invalidated' }
    }
    if ((Get-Item $env:OPENSSL_SRC_PERL).LastWriteTimeUtc.Ticks -ne $adapterTime) { throw 'Cached adapter was recompiled' }
    if (@([regex]::Matches($env:CXXFLAGS, '(?i)(?:^|\s)/EHsc(?=\s|$)')).Count -ne 1) { throw 'Exception flag was duplicated' }
    $marker = Get-ChildItem -Path "$fixture/target/debug/build/openssl-sys-*/out/tideterm-adapter-version" -File | Select-Object -First 1
    if (-not $marker) { throw 'Missing fixture configuration marker' }
    Set-Content -LiteralPath $marker.FullName -Value 'previous-adapter.exe'
    . (Join-Path $fixtureScripts 'prepare-windows-build.ps1')
    if (Test-Path -LiteralPath $cache[0].path) { throw 'Changed adapter marker did not invalidate OpenSSL' }
    foreach ($file in $bystanders) {
        if ((Get-FileHash -LiteralPath $file.path).Hash -ne $file.hash -or
            (Get-Item $file.path).LastWriteTimeUtc.Ticks -ne $file.time) { throw 'Version migration changed an unrelated artifact' }
    }
    Write-Output "PASS: both profiles migrated, matching cache/adapter retained, changed version invalidated, unrelated artifacts preserved. Fixture: $fixture"
} finally {
    $env:CARGO_TARGET_DIR = $savedTarget
    $env:OPENSSL_SRC_PERL = $savedPerl
}
