[CmdletBinding()]
param(
    [string] $ArchivePath,
    [ValidateSet('x64', 'arm64')][string] $Architecture = 'x64'
)

$ErrorActionPreference = 'Stop'
$prepare = Join-Path $PSScriptRoot '../prepare-windows-runtime.ps1'
$root = Join-Path ([IO.Path]::GetTempPath()) "pebrel-runtime-test-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $root | Out-Null
try {
    $destination = Join-Path $root 'runtime'
    & $prepare -Destination $destination -ArchivePath $ArchivePath -Architecture $Architecture
    $before = @(Get-ChildItem $destination | Sort-Object Name | ForEach-Object { $_.LastWriteTimeUtc.Ticks })
    & $prepare -Destination $destination -ArchivePath $ArchivePath -Architecture $Architecture
    $after = @(Get-ChildItem $destination | Sort-Object Name | ForEach-Object { $_.LastWriteTimeUtc.Ticks })
    if (@(Compare-Object $before $after).Count -ne 0) { throw 'Verified runtime files were unnecessarily replaced.' }
    if (@(Get-ChildItem $destination).Count -ne 2) { throw 'Unexpected files were extracted.' }
    $dll = Join-Path $destination 'conpty.dll'
    $originalHash = (Get-FileHash $dll -Algorithm SHA256).Hash
    [IO.File]::WriteAllBytes($dll, [byte[]]@(0, 1, 2, 3))
    & $prepare -Destination $destination -ArchivePath $ArchivePath -Architecture $Architecture
    if ((Get-FileHash $dll -Algorithm SHA256).Hash -ne $originalHash) {
        throw 'A corrupt or wrong-architecture installed runtime was reused.'
    }

    $invalid = Join-Path $root 'different.zip'
    [IO.File]::WriteAllBytes($invalid, [byte[]]@(80, 75, 0, 0))
    $rejected = $false
    try { & $prepare -Destination (Join-Path $root 'rejected') -ArchivePath $invalid -Architecture $Architecture }
    catch {
        if ($_.Exception.Message -notlike '*SHA256 verification*') { throw }
        $rejected = $true
    }
    if (-not $rejected) { throw 'A different archive was accepted.' }
    if (Test-Path (Join-Path $root 'rejected')) { throw 'Files were created before archive verification.' }
    if ($ArchivePath) {
        $otherArchitecture = if ($Architecture -eq 'arm64') { 'x64' } else { 'arm64' }
        $rejected = $false
        try { & $prepare -Destination (Join-Path $root 'wrong-arch') -ArchivePath $ArchivePath -Architecture $otherArchitecture }
        catch {
            if ($_.Exception.Message -notlike '*SHA256 verification*') { throw }
            $rejected = $true
        }
        if (-not $rejected) { throw 'An archive for the other architecture was accepted.' }
        if (Test-Path (Join-Path $root 'wrong-arch')) { throw 'A wrong-architecture archive was extracted.' }
    }
    Write-Output "windows-runtime.tests.ps1: PASS ($Architecture pinned files, reuse, repair, rejected archives)"
} finally { Remove-Item -LiteralPath $root -Recurse -Force }
