[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string] $InstallationDirectory,
    [switch] $Unregister
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$installation = (Resolve-Path -LiteralPath $InstallationDirectory).Path
$helper = Join-Path $installation 'context-menu/pebrel-context-menu-setup.exe'
$operation = if ($Unregister) { '--unregister' } else { '--register' }
& $helper $operation
if ($LASTEXITCODE -ne 0) {
    throw "Context-menu deployment failed; see $installation/context-menu/setup.log"
}
