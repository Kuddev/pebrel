[CmdletBinding()]
param([Parameter(Mandatory = $true)][string] $InnoCompiler)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repo = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../..')).Path
$root = Join-Path $repo ('tmp/installer-context-menu-' + [guid]::NewGuid().ToString('N'))
$name = 'Pebrel.ContextMenu.Test.' + [guid]::NewGuid().ToString('N').Substring(0, 12)
New-Item -ItemType Directory -Path $root | Out-Null
$payload = Join-Path $root 'payload'
Copy-Item -LiteralPath (Join-Path $repo 'target/release/context-menu') -Destination $payload -Recurse
$path = Join-Path $payload 'AppxManifest.xml'
$manifest = [IO.File]::ReadAllText($path).Replace('Name="Pebrel.ContextMenu"', "Name=`"$name`"")
$manifest = $manifest.Replace('209e5492-2c19-45c0-ae7e-a4beffd09073', [guid]::NewGuid().ToString())
[IO.File]::WriteAllText($path, $manifest, [Text.UTF8Encoding]::new($false))
try {
    & $InnoCompiler '/Q' "/DFixtureRoot=$root" "/DFixtureIdentity=$name" "/O$root" (Join-Path $PSScriptRoot 'installer-context-menu-fixture.iss')
    if ($LASTEXITCODE -ne 0) { throw 'Context menu installer fixture failed to compile' }
    for ($i = 0; $i -lt 2; $i++) {
        $process = Start-Process -FilePath (Join-Path $root 'installer-context-menu-fixture.exe') -WindowStyle Hidden -Wait -PassThru -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/SP-', "/LOG=`"$root/install-$i.log`"")
        if ($process.ExitCode -ne 0) { throw "Installer failed: $($process.ExitCode)" }
        if (-not (Get-AppxPackage -Name $name)) { throw 'Installer did not automatically register the menu' }
    }
    $uninstaller = Join-Path $root 'installed/unins000.exe'
    $process = Start-Process -FilePath $uninstaller -WindowStyle Hidden -Wait -PassThru -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/CancelBeforeConfirmation=1')
    if (-not (Get-AppxPackage -Name $name)) { throw 'Cancelled uninstall removed the menu' }
    $installedManifest = Join-Path $root 'installed/context-menu/AppxManifest.xml'
    $originalManifest = [IO.File]::ReadAllText($installedManifest)
    [IO.File]::WriteAllText($installedManifest, '<Package/>')
    $process = Start-Process -FilePath $uninstaller -WindowStyle Hidden -Wait -PassThru -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART')
    if ($process.ExitCode -eq 0) { throw 'Uninstaller accepted failed package cleanup' }
    if (-not (Get-AppxPackage -Name $name)) { throw 'Failed uninstall removed the menu' }
    if (-not (Test-Path -LiteralPath (Join-Path $root 'installed/context-menu/pebrel_context_menu.dll'))) { throw 'Failed uninstall removed the DLL' }
    [IO.File]::WriteAllText($installedManifest, $originalManifest, [Text.UTF8Encoding]::new($false))
    $process = Start-Process -FilePath $uninstaller -WindowStyle Hidden -Wait -PassThru -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART')
    if ($process.ExitCode -ne 0) { throw "Uninstaller failed: $($process.ExitCode)" }
    if (Get-AppxPackage -Name $name) { throw 'Uninstaller left the package registered' }
    if (Test-Path -LiteralPath (Join-Path $root 'installed/context-menu/pebrel_context_menu.dll')) { throw 'Uninstaller left the DLL' }
    Write-Output 'installer-context-menu.tests.ps1: PASS (install, reinstall, cancellation, failed cleanup, unregister and payload removal)'
} finally {
    Get-AppxPackage -Name $name | Remove-AppxPackage
    Write-Output "Isolated installer fixture: $root"
}
