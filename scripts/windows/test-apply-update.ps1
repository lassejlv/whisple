# Exercise the actual helper against disposable files, without launching apps.
$ErrorActionPreference = 'Stop'
$helper = Join-Path $PSScriptRoot 'apply-update.ps1'
function Get-Process { param($Id, $ErrorAction) return $null }
function Start-Process {
    param($FilePath)
    $global:WhispleTestLaunches++
    if ($global:WhispleTestFailLaunch -and $global:WhispleTestLaunches -eq 1) {
        throw 'Simulated launch failure'
    }
}

foreach ($scenario in @('success', 'missing-source', 'launch-failure')) {
    $root = Join-Path ([System.IO.Path]::GetTempPath()) ("whisple update test " + [guid]::NewGuid())
    $work = Join-Path $root 'work'
    New-Item -ItemType Directory -Path $work -Force | Out-Null
    $target = Join-Path $root 'whisple.exe'
    $staged = Join-Path $work 'whisple.exe'
    [System.IO.File]::WriteAllText($target, 'old executable')
    if ($scenario -ne 'missing-source') { [System.IO.File]::WriteAllText($staged, 'new executable') }
    # A failed copy must never restore an even older backup over the current app.
    [System.IO.File]::WriteAllText("$target.previous", 'stale backup')
    $global:WhispleTestLaunches = 0
    $global:WhispleTestFailLaunch = $scenario -eq 'launch-failure'
    try {
        & $helper -Target $target -Staged $staged -OldPid 2147483647 -WorkDir $work -Version '1.2.3' 2>$null
        $expected = if ($scenario -eq 'success') { 'new executable' } else { 'old executable' }
        if ([System.IO.File]::ReadAllText($target) -ne $expected) { throw "Wrong executable after $scenario" }
        if ($scenario -eq 'success' -and ((Test-Path "$target.previous") -or (Test-Path $work))) {
            throw 'Successful update did not clean up its backup and staging folder.'
        }
        if (Get-ChildItem -Path $root -Filter '*.update-*') { throw 'Staged replacement was left behind.' }
        Write-Output "Updater helper verified: $scenario"
    } finally {
        Remove-Item -LiteralPath $root -Recurse -Force
    }
}
# Expected helper failures set LASTEXITCODE, so clear it after assertions pass.
exit 0
