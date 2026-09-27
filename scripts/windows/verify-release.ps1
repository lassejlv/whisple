# Verify the release payload; optionally install and uninstall both packages
# on a clean Windows machine. Existing installations are never overwritten.
param([switch] $TestInstallers)
$ErrorActionPreference = 'Stop'

$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$version = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version = "(.+)"' |
    Select-Object -First 1).Matches[0].Groups[1].Value
$prefix = "Whisple-$version-windows-x86_64"
$dist = Join-Path $root 'target\dist'
$expected = @("$prefix-setup.exe", "$prefix.msi", "$prefix.zip")
$seen = @{}
foreach ($line in Get-Content -LiteralPath (Join-Path $dist "$prefix.sha256")) {
    if ($line -cnotmatch '^([0-9a-f]{64})  (.+)$') { throw "Invalid checksum entry: $line" }
    $digest, $name = $Matches[1], $Matches[2]
    if ($expected -cnotcontains $name -or $seen.ContainsKey($name)) {
        throw "Unexpected or duplicate checksum entry: $name"
    }
    $asset = Get-Item -LiteralPath (Join-Path $dist $name)
    if ($asset.Length -eq 0) { throw "Empty release asset: $name" }
    if ((Get-FileHash -LiteralPath $asset.FullName -Algorithm SHA256).Hash.ToLowerInvariant() -cne $digest) {
        throw "Checksum mismatch: $name"
    }
    $seen[$name] = $true
}
if ($seen.Count -ne $expected.Count) { throw 'The checksum manifest is incomplete.' }

$work = Join-Path ([System.IO.Path]::GetTempPath()) ("whisple-release-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $work | Out-Null
try {
    Expand-Archive -LiteralPath (Join-Path $dist "$prefix.zip") -DestinationPath $work
    $exe = Join-Path $work 'whisple.exe'
    $files = @(Get-ChildItem -LiteralPath $work -File -Recurse)
    if ($files.Count -ne 1 -or $files[0].FullName -ne $exe) {
        throw 'The updater ZIP must contain only whisple.exe at its root.'
    }
    if ((Get-Item -LiteralPath $exe).VersionInfo.ProductVersion -cne $version) {
        throw 'The updater executable has the wrong version.'
    }
    $reader = [System.IO.BinaryReader]::new([System.IO.File]::OpenRead($exe))
    try {
        if ($reader.ReadUInt16() -ne 0x5a4d) { throw 'Missing DOS executable header.' }
        $reader.BaseStream.Position = 0x3c
        $peOffset = $reader.ReadUInt32()
        $reader.BaseStream.Position = $peOffset
        if ($reader.ReadUInt32() -ne 0x4550 -or $reader.ReadUInt16() -ne 0x8664) {
            throw 'The updater payload is not a Windows x86_64 executable.'
        }
    } finally { $reader.Dispose() }
    $payloadHash = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash
    $source = Join-Path $root 'target\release\whisple.exe'
    if ((Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash -ne $payloadHash) {
        throw 'The updater ZIP differs from the compiled binary.'
    }
    Write-Output 'All checksums, updater layout, version, architecture, and binary contents verified.'

    if ($TestInstallers) {
        if ($env:OS -ne 'Windows_NT') { throw 'Installer tests require Windows.' }
        $installDir = Join-Path $env:LOCALAPPDATA 'Programs\Whisple'
        $installedExe = Join-Path $installDir 'whisple.exe'
        $shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'Whisple.lnk'
        if ((Test-Path -LiteralPath $installDir) -or (Test-Path -LiteralPath $shortcut)) {
            throw 'Installer tests require a clean machine without Whisple installed.'
        }
        $logs = Join-Path $root 'target\windows-smoke-logs'
        New-Item -ItemType Directory -Force -Path $logs | Out-Null

        function Invoke-Installer([string] $FilePath, [string[]] $Arguments) {
            $process = Start-Process -FilePath $FilePath -ArgumentList $Arguments -Wait -PassThru
            if ($process.ExitCode -notin @(0, 3010)) {
                throw "Installer failed with exit code $($process.ExitCode): $FilePath"
            }
        }
        function Assert-Installed {
            if ((Get-FileHash -LiteralPath $installedExe -Algorithm SHA256).Hash -ne $payloadHash) {
                throw 'The installed binary differs from the updater payload.'
            }
            if (-not (Test-Path -LiteralPath $shortcut)) { throw 'The Start menu shortcut is missing.' }
        }
        function Assert-Uninstalled {
            if ((Test-Path -LiteralPath $installedExe) -or (Test-Path -LiteralPath $shortcut)) {
                throw 'Uninstall left the executable or Start menu shortcut behind.'
            }
        }

        $uninstaller = Join-Path $installDir 'unins000.exe'
        try {
            Invoke-Installer (Join-Path $dist "$prefix-setup.exe") @(
                '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/SP-',
                "/LOG=`"$(Join-Path $logs 'inno-install.log')`"")
            Assert-Installed
        } finally {
            if (Test-Path -LiteralPath $uninstaller) {
                Invoke-Installer $uninstaller @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART')
            }
        }
        Assert-Uninstalled
        Write-Output 'Inno Setup install, payload, shortcut, and uninstall verified.'

        $msi = Join-Path $dist "$prefix.msi"
        $msiexec = Join-Path $env:WINDIR 'System32\msiexec.exe'
        Invoke-Installer $msiexec @('/i', "`"$msi`"", '/qn', '/norestart',
            '/L*v', "`"$(Join-Path $logs 'msi-install.log')`"")
        try {
            Assert-Installed
        } finally {
            Invoke-Installer $msiexec @('/x', "`"$msi`"", '/qn', '/norestart',
                '/L*v', "`"$(Join-Path $logs 'msi-uninstall.log')`"")
        }
        Assert-Uninstalled
        Write-Output 'MSI install, payload, shortcut, and uninstall verified.'
    }
} finally {
    Remove-Item -LiteralPath $work -Recurse -Force
}
