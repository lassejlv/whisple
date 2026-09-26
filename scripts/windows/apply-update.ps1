# Runs outside Whisple after the app quits. The previous whisple.exe is kept
# until the update is in place, and restored if anything fails.
param(
    [Parameter(Mandatory)] [string] $Target,
    [Parameter(Mandatory)] [string] $Staged,
    [Parameter(Mandatory)] [int] $OldPid,
    [Parameter(Mandatory)] [string] $WorkDir,
    [Parameter(Mandatory)] [string] $Version
)
$ErrorActionPreference = 'Stop'

$deadline = (Get-Date).AddSeconds(20)
while (Get-Process -Id $OldPid -ErrorAction SilentlyContinue) {
    if ((Get-Date) -gt $deadline) {
        Write-Error 'Whisple did not quit; the existing app was left untouched.'
        exit 1
    }
    Start-Sleep -Milliseconds 100
}

$backup = "$Target.previous"
try {
    if (Test-Path -LiteralPath $backup) {
        Remove-Item -LiteralPath $backup -Force
    }
    Move-Item -LiteralPath $Target -Destination $backup
    Copy-Item -LiteralPath $Staged -Destination $Target
} catch {
    Write-Error $_ -ErrorAction Continue
    if (-not (Test-Path -LiteralPath $Target) -and (Test-Path -LiteralPath $backup)) {
        Move-Item -LiteralPath $backup -Destination $Target
    }
    if (Test-Path -LiteralPath $Target) {
        Start-Process -FilePath $Target
    }
    exit 1
}

# Keep Settings › Apps showing the running version for the Inno Setup install.
# The MSI records its version in Windows Installer and is left as is.
$uninstall = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{5E0F9D9B-7C1A-4E5B-9E54-3A0C6B3F2D71}_is1'
if (Test-Path -LiteralPath $uninstall) {
    Set-ItemProperty -LiteralPath $uninstall -Name DisplayVersion -Value $Version -ErrorAction SilentlyContinue
}

Start-Process -FilePath $Target
Remove-Item -LiteralPath $backup -Force -ErrorAction SilentlyContinue
Remove-Item -LiteralPath $WorkDir -Recurse -Force -ErrorAction SilentlyContinue
