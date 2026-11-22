<#
.SYNOPSIS
Builds the per-user Camo Translator installer for Windows 10/11 x64.

.DESCRIPTION
Use PowerShell with the Rust MSVC toolchain and Windows SDK available.
Pass paths to a portable NSIS distribution, UPX, and the unmodified Noto
Sans CJK SC Regular OTF with its OFL.txt from the Sans2.004 release.
#>

[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$MakeNsis,
    [Parameter(Mandatory)][string]$Upx,
    [Parameter(Mandatory)][string]$FontDirectory,
    [Parameter(Mandatory)][string]$OutputDirectory,
    [string]$BuildDirectory = (Join-Path $PSScriptRoot '../../.dev/windows-setup'),
    [switch]$Offline
)

$ErrorActionPreference = 'Stop'
$source = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../..')).ProviderPath
$makeNsis = (Resolve-Path -LiteralPath $MakeNsis).ProviderPath
$upx = (Resolve-Path -LiteralPath $Upx).ProviderPath
$font = (Resolve-Path -LiteralPath $FontDirectory).ProviderPath
$nsisLicense = Join-Path (Split-Path -Parent $makeNsis) 'COPYING'
$upxLicense = Join-Path (Split-Path -Parent $upx) 'LICENSE'
$target = 'x86_64-pc-windows-msvc'
$build = (New-Item -ItemType Directory -Force -Path $BuildDirectory).FullName
$destination = (New-Item -ItemType Directory -Force -Path $OutputDirectory).FullName
$cargoTarget = Join-Path $build 'target'
$cargoOptions = @('--locked')
if ($Offline) {
    $cargoOptions += '--offline'
}

Push-Location -LiteralPath $source
$savedFlags = $env:CARGO_ENCODED_RUSTFLAGS
try {
    $metadataJson = & cargo metadata @cargoOptions --format-version 1 --filter-platform $target
    if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed.' }
    $metadata = $metadataJson | ConvertFrom-Json
    $version = ($metadata.packages | Where-Object id -eq $metadata.resolve.root).version

    # An explicit target keeps these flags off host-only procedural macros.
    $env:CARGO_ENCODED_RUSTFLAGS = "-C$([char]31)target-feature=+crt-static"
    & cargo build @cargoOptions --release --target $target --bins --target-dir $cargoTarget
    if ($LASTEXITCODE -ne 0) { throw 'The Windows x64 release build failed.' }
} finally {
    $env:CARGO_ENCODED_RUSTFLAGS = $savedFlags
    Pop-Location
}

$notices = Join-Path $build 'Third-party notices.txt'
$noticeWriter = [IO.StreamWriter]::new($notices, $false, [Text.UTF8Encoding]::new($true))
try {
    foreach ($package in ($metadata.packages | Where-Object source | Sort-Object name, version)) {
        $noticeWriter.WriteLine("=== $($package.name) $($package.version) ===")
        $noticeWriter.WriteLine($package.license)
        $noticeWriter.WriteLine($package.repository)
        $packageDir = Split-Path -Parent $package.manifest_path
        $licenseFiles = @(Get-ChildItem -LiteralPath $packageDir -File |
            Where-Object Name -Match '^(licen[sc]e|notice|copyright|copying|ofl)($|[. _-])' |
            Select-Object -ExpandProperty FullName)
        if ($package.license_file) {
            $licenseFiles += Join-Path $packageDir $package.license_file
        }
        if ($package.name -eq 'epaint_default_fonts') {
            $licenseFiles += Get-ChildItem -LiteralPath (Join-Path $packageDir 'fonts') -Filter '*.txt' -File |
                Select-Object -ExpandProperty FullName
        }
        foreach ($licenseFile in ($licenseFiles | Sort-Object -Unique)) {
            $noticeWriter.WriteLine([IO.File]::ReadAllText($licenseFile))
        }
        $noticeWriter.WriteLine()
    }

    $noticeWriter.WriteLine('=== Noto Sans CJK SC Regular 2.004 ===')
    $noticeWriter.WriteLine('Copyright 2014-2021 Adobe (http://www.adobe.com/). Noto is a trademark of Google Inc.')
    $noticeWriter.WriteLine([IO.File]::ReadAllText((Join-Path $font 'OFL.txt')))
    $noticeWriter.WriteLine('=== NSIS ===')
    $noticeWriter.WriteLine([IO.File]::ReadAllText($nsisLicense))
    $noticeWriter.WriteLine('=== UPX ===')
    $noticeWriter.WriteLine([IO.File]::ReadAllText($upxLicense))
} finally {
    $noticeWriter.Dispose()
}

$setupFile = Join-Path $destination "Camo-Translator-$version-Windows-x64-Setup.exe"
$release = Join-Path $cargoTarget "$target/release"
$nsisOptions = @(
    '/V3', '/WX', '/INPUTCHARSET', 'UTF8',
    "/DVERSION=$version",
    "/DSETUP_EXE=$setupFile",
    "/DUPX_EXE=$upx",
    "/DFONT_DIR=$font",
    "/DRELEASE_DIR=$release",
    "/DUSAGE_FILE=$(Join-Path $PSScriptRoot 'readme.txt')",
    "/DPRODUCT_LICENSE=$(Join-Path $source 'LICENSE')",
    "/DPRODUCT_ICON=$(Join-Path $source 'assets/camo-translator.ico')",
    "/DNOTICE_FILE=$notices"
)
& $makeNsis @nsisOptions (Join-Path $PSScriptRoot 'setup.nsi')
if ($LASTEXITCODE -ne 0) { throw 'NSIS failed to build the installer.' }
Get-FileHash -LiteralPath $setupFile -Algorithm SHA256
