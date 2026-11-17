[CmdletBinding()]
param(
    [ValidateSet('debug', 'release')]
    [string] $Configuration = 'release',
    [string] $TargetDirectory,
    [switch] $ContextMenu,
    [string] $ContextMenuPackage
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
if ([string]::IsNullOrWhiteSpace($TargetDirectory)) {
    $TargetDirectory = Join-Path $repo 'target'
} elseif (-not [IO.Path]::IsPathRooted($TargetDirectory)) {
    $TargetDirectory = Join-Path $repo $TargetDirectory
}
$previousTargetDirectory = $env:CARGO_TARGET_DIR
Push-Location $repo
try {
    $env:CARGO_TARGET_DIR = [IO.Path]::GetFullPath($TargetDirectory)
    # Keep the feature graph identical for the initial build, ZIP and installer.
    # The acceptance lab is tested by CI but does not supply a packaged binary.
    $arguments = @(
        'build', '--locked', '--timings',
        '-p', 'pebrel', '--bin', 'pebrel', '--bin', 'pebrel-cli',
        '-p', 'pebrel_hook', '--bin', 'pebrel-hook',
        '--features', 'pebrel/gpui-shell'
    )
    if ($Configuration -eq 'release') { $arguments += '--release' }
    & cargo @arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Cargo Windows product build failed with exit code $LASTEXITCODE"
    }
    if ($ContextMenu -or $ContextMenuPackage) {
        & (Join-Path $PSScriptRoot 'build-windows-context-menu.ps1') -Configuration $Configuration -TargetDirectory $TargetDirectory -SignedPackage $ContextMenuPackage
    }
} finally {
    $env:CARGO_TARGET_DIR = $previousTargetDirectory
    Pop-Location
}
