# Builds the Windows release assets into target/dist:
#
#   Whisple-<version>-windows-x86_64-setup.exe  Inno Setup installer
#   Whisple-<version>-windows-x86_64.msi        Windows Installer package
#   Whisple-<version>-windows-x86_64.zip        whisple.exe for the updater
#   Whisple-<version>-windows-x86_64.sha256     checksums of the three
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts/windows/package-windows.ps1 [-WithLicensing] [-SkipBuild]
#
# Needs Inno Setup 6 (ISCC.exe) and the WiX v5 .NET tool (wix.exe).
param(
    [switch] $WithLicensing,
    [switch] $SkipBuild,
    [string] $Iscc,
    [string] $Wix
)
$ErrorActionPreference = 'Stop'

$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$version = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version = "(.+)"' |
    Select-Object -First 1).Matches[0].Groups[1].Value
# Installers need a numeric version; a prerelease shares its release's number.
$numeric = ($version -split '[-+]')[0]
$arch = 'x86_64'
$prefix = "Whisple-$version-windows-$arch"

function Find-Tool([string] $explicit, [string] $name, [string[]] $candidates) {
    if ($explicit) { return $explicit }
    $onPath = Get-Command $name -ErrorAction SilentlyContinue
    if ($onPath) { return $onPath.Source }
    foreach ($candidate in $candidates) {
        if ($candidate -and (Test-Path -LiteralPath $candidate)) { return $candidate }
    }
    throw "Could not find $name. Install it or pass its path."
}
$Iscc = Find-Tool $Iscc 'ISCC.exe' @(
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
)
$Wix = Find-Tool $Wix 'wix.exe' @("$env:USERPROFILE\.dotnet\tools\wix.exe")

Push-Location $root
try {
    if (-not $SkipBuild) {
        $features = if ($WithLicensing) { @('--features', 'licensing') } else { @() }
        & cargo build --release --locked @features
        if ($LASTEXITCODE -ne 0) { throw 'cargo build failed.' }
    }
    $exe = Join-Path $root 'target\release\whisple.exe'
    $embedded = (Get-Item -LiteralPath $exe).VersionInfo.ProductVersion
    if ($embedded -ne $version) {
        throw "whisple.exe reports version $embedded, expected $version."
    }

    $dist = Join-Path $root 'target\dist'
    New-Item -ItemType Directory -Force -Path $dist | Out-Null
    $zip = Join-Path $dist "$prefix.zip"
    $setup = Join-Path $dist "$prefix-setup.exe"
    $msi = Join-Path $dist "$prefix.msi"
    foreach ($old in $zip, $setup, $msi) {
        if (Test-Path -LiteralPath $old) { Remove-Item -LiteralPath $old -Force }
    }

    # The updater unpacks whisple.exe from the archive root.
    Compress-Archive -LiteralPath $exe -DestinationPath $zip -CompressionLevel Optimal

    & $Iscc /Q "/DVersion=$version" "/DNumericVersion=$numeric.0" "/DSourceExe=$exe" `
        "/DOutputDir=$dist" "/DOutputBase=$prefix-setup" (Join-Path $PSScriptRoot 'whisple.iss')
    if ($LASTEXITCODE -ne 0) { throw 'Inno Setup failed.' }

    & $Wix build -arch x64 -nologo `
        -d "Version=$version" -d "NumericVersion=$numeric" -d "SourceExe=$exe" `
        -d "IconFile=$(Join-Path $root 'assets\whisple.ico')" `
        -o $msi (Join-Path $PSScriptRoot 'whisple.wxs')
    if ($LASTEXITCODE -ne 0) { throw 'WiX failed.' }
    Remove-Item -LiteralPath ([System.IO.Path]::ChangeExtension($msi, '.wixpdb')) -Force -ErrorAction SilentlyContinue

    # The same "<hash>  <name>" lines shasum writes for the macOS assets.
    $lines = foreach ($asset in $setup, $msi, $zip) {
        $hash = (Get-FileHash -LiteralPath $asset -Algorithm SHA256).Hash.ToLowerInvariant()
        "$hash  $(Split-Path -Leaf $asset)"
    }
    [System.IO.File]::WriteAllText((Join-Path $dist "$prefix.sha256"), (($lines -join "`n") + "`n"))

    $setup
    $msi
    $zip
    Join-Path $dist "$prefix.sha256"
} finally {
    Pop-Location
}
