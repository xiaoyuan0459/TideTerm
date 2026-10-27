param([string]$QtDir = $env:QT_DIR)
$ErrorActionPreference = 'Stop'
if (-not $QtDir -or -not (Test-Path -LiteralPath (Join-Path $QtDir 'include/QtGui'))) {
    throw 'Set QT_DIR to the Qt MSVC kit before rendering icons.'
}
$nativeRoot = Split-Path -Parent $PSScriptRoot
$outputDir = Join-Path $nativeRoot 'target/diagnostics/icons'
New-Item -ItemType Directory -Path $outputDir -Force | Out-Null
$vswhere = 'C:/Program Files (x86)/Microsoft Visual Studio/Installer/vswhere.exe'
$vsInstall = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
$vcTools = Get-ChildItem -LiteralPath (Join-Path $vsInstall 'VC/Tools/MSVC') -Directory | Sort-Object Name | Select-Object -Last 1
$sdkInclude = Get-ChildItem -LiteralPath 'C:/Program Files (x86)/Windows Kits/10/Include' -Directory | Sort-Object Name | Select-Object -Last 1
$sdkLib = Join-Path 'C:/Program Files (x86)/Windows Kits/10/Lib' $sdkInclude.Name
$env:INCLUDE = @((Join-Path $vcTools.FullName 'include'), (Join-Path $sdkInclude.FullName 'ucrt'),
    (Join-Path $sdkInclude.FullName 'um'), (Join-Path $sdkInclude.FullName 'shared')) -join ';'
$env:LIB = @((Join-Path $vcTools.FullName 'lib/x64'), (Join-Path $sdkLib 'ucrt/x64'),
    (Join-Path $sdkLib 'um/x64'), (Join-Path $QtDir 'lib')) -join ';'
$vcBin = Join-Path $vcTools.FullName 'bin/Hostx64/x64'
$env:PATH = "$vcBin;$(Join-Path $QtDir 'bin');$env:PATH"
$sources = @('tools/icon_gallery.cpp', 'cpp/chrome/icon_shapes.cpp',
    'cpp/chrome/icon_shapes_actions.cpp', 'cpp/chrome/icon_shapes_files.cpp') |
    ForEach-Object { Join-Path $nativeRoot $_ }
$exe = Join-Path $outputDir 'icon-gallery.exe'
Push-Location $outputDir
try {
    & (Join-Path $vcBin 'cl.exe') /nologo /EHsc /MD /std:c++17 /utf-8 /Zc:__cplusplus /permissive- `
        "/I$nativeRoot" "/I$(Join-Path $QtDir 'include')" "/I$(Join-Path $QtDir 'include/QtCore')" `
        "/I$(Join-Path $QtDir 'include/QtGui')" $sources "/Fe:$exe" /link Qt6Gui.lib Qt6Core.lib
    if ($LASTEXITCODE -ne 0) { throw "Icon gallery compilation failed: $LASTEXITCODE" }
    $env:QT_QPA_PLATFORM = 'offscreen'
    & $exe (Join-Path $nativeRoot 'assets/icons/chrome-icons.tsv') $outputDir
    if ($LASTEXITCODE -ne 0) { throw "Icon gallery rendering failed: $LASTEXITCODE" }
} finally { Pop-Location }
