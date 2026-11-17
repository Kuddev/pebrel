[CmdletBinding()]
param([string] $PayloadDirectory = 'target/release/context-menu')
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repo = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../..')).Path
$payload = (Resolve-Path -LiteralPath (Join-Path $repo $PayloadDirectory)).Path
$root = Join-Path $repo ('tmp/context-menu-test-' + [guid]::NewGuid().ToString('N'))
$name = 'Pebrel.ContextMenu.Test.' + [guid]::NewGuid().ToString('N').Substring(0, 12)
$clsid = [guid]::NewGuid().ToString()
$first = Join-Path $root '安装 A #1'
$second = Join-Path $root '安装 B'
foreach ($directory in @($first, $second)) {
    New-Item -ItemType Directory -Path $directory -Force | Out-Null
    Copy-Item -LiteralPath $payload -Destination (Join-Path $directory 'context-menu') -Recurse
    Copy-Item -LiteralPath (Join-Path $payload 'pebrel-context-menu-setup.exe') -Destination (Join-Path $directory 'pebrel.exe')
    $path = Join-Path $directory 'context-menu/AppxManifest.xml'
    $manifest = [IO.File]::ReadAllText($path).Replace('Name="Pebrel.ContextMenu"', "Name=`"$name`"")
    $manifest = $manifest.Replace('209e5492-2c19-45c0-ae7e-a4beffd09073', $clsid)
    [IO.File]::WriteAllText($path, $manifest, [Text.UTF8Encoding]::new($false))
}
function Invoke-Helper([string] $Directory, [string] $Operation, [bool] $Success = $true) {
    & (Join-Path $Directory 'context-menu/pebrel-context-menu-setup.exe') $Operation
    if (($LASTEXITCODE -eq 0) -ne $Success) {
        throw "Unexpected exit $LASTEXITCODE for $Operation in $Directory"
    }
}
try {
    Invoke-Helper $first '--register'
    Invoke-Helper $first '--register'
    $package = Get-AppxPackage -Name $name
    if ($null -eq $package -or $package.Status -ne 'Ok') { throw 'Registration not healthy' }
    $path = Join-Path $first 'context-menu/AppxManifest.xml'
    [xml] $manifest = [IO.File]::ReadAllText($path)
    $manifest.Package.Identity.Version = '99.0.0.0'
    $manifest.Save($path)
    Invoke-Helper $first '--register'
    if ((Get-AppxPackage -Name $name).Version -ne '99.0.0.0') { throw 'Upgrade failed' }
    Invoke-Helper $second '--register' $false
    Invoke-Helper $second '--unregister'
    if (-not (Get-AppxPackage -Name $name)) { throw 'Foreign uninstall removed the package' }
    $invalidSigned = Join-Path $first 'context-menu/Pebrel.ContextMenu.msix'
    [IO.File]::WriteAllText($invalidSigned, 'invalid signed package')
    Invoke-Helper $first '--register' $false
    if (-not (Get-AppxPackage -Name $name)) { throw 'Failed deployment removed the prior registration' }
    Invoke-Helper $first '--unregister'
    Invoke-Helper $first '--unregister'
    if (Get-AppxPackage -Name $name) { throw 'Uninstall left the package registered' }
    Write-Output 'windows-context-menu.tests.ps1: PASS (register, repeat, upgrade, ownership, signed failure, unregister)'
} finally {
    # The random package identity and CLSID cannot address the real installation.
    Get-AppxPackage -Name $name | Remove-AppxPackage
    Write-Output "Isolated fixture: $root"
}
