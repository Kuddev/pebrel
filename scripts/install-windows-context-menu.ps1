[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string] $InstallationDirectory,
    [switch] $Unregister
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$installation = (Resolve-Path -LiteralPath $InstallationDirectory).Path
$packageRoot = Join-Path $installation 'context-menu'
$manifestPath = Join-Path $packageRoot 'AppxManifest.xml'
$existing = Get-AppxPackage -Name 'Pebrel.ContextMenu'
if ($Unregister) {
    if ($null -eq $existing) { return }
    if ($existing.InstallLocation -ne $packageRoot) {
        throw "Context menu belongs to another directory: $($existing.InstallLocation)"
    }
    Remove-AppxPackage -Package $existing.PackageFullName
    if (Get-AppxPackage -Name 'Pebrel.ContextMenu') { throw 'Context menu unregistration failed' }
    Write-Output 'Explorer context menu unregistered.'
    return
}
foreach ($file in @('pebrel.exe', 'context-menu/pebrel_context_menu.dll', 'context-menu/assets/logo.png', 'context-menu/AppxManifest.xml')) {
    if (-not (Test-Path -LiteralPath (Join-Path $installation $file) -PathType Leaf)) {
        throw "Missing context-menu input: $file"
    }
}
[xml] $manifest = [IO.File]::ReadAllText($manifestPath)
if ($manifest.Package.Identity.Name -ne 'Pebrel.ContextMenu' -or $manifest.Package.Identity.Publisher -ne 'CN=PebrelLocal') {
    throw 'Unexpected Explorer package identity'
}
# Explicit per-user development registration. This does not enable Developer
# Mode, import certificates, change machine policy or install a Store package.
Add-AppxPackage -Register $manifestPath -ExternalLocation $installation
$registered = Get-AppxPackage -Name 'Pebrel.ContextMenu'
if ($null -eq $registered -or $registered.InstallLocation -ne $packageRoot -or $registered.Status -ne 'Ok') {
    throw 'Explorer package registration did not match the requested installation'
}
$registered | Select-Object Name,Version,InstallLocation,Status
