$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$builder = Join-Path $PSScriptRoot '../build-windows-product.ps1'
$originalTarget = $env:CARGO_TARGET_DIR
$originalDirectory = (Get-Location).Path
$global:ProductBuildCalls = @()
$global:ProductBuildExit = 0
function global:cargo {
    $global:ProductBuildCalls += ,@($args)
    $global:LASTEXITCODE = $global:ProductBuildExit
}
try {
    $env:CARGO_TARGET_DIR = 'preserve-caller-target'
    & $builder -Configuration release -TargetDirectory target
    if ($global:ProductBuildCalls.Count -ne 1) { throw 'Expected one product build' }
    $arguments = $global:ProductBuildCalls[0]
    $packages = @(); $binaries = @()
    for ($i = 0; $i -lt $arguments.Count - 1; $i++) {
        if ($arguments[$i] -eq '-p') { $packages += $arguments[$i + 1] }
        if ($arguments[$i] -eq '--bin') { $binaries += $arguments[$i + 1] }
    }
    if (($packages -join ',') -ne 'nebula,nebula_hook') { throw 'Unexpected package selection' }
    if (($binaries -join ',') -ne 'pebrel,pebrel-hook') { throw 'Unexpected product binaries' }
    foreach ($required in @('--locked', '--release', '--features', 'nebula/gpui-shell')) {
        if ($required -notin $arguments) { throw "Missing product build argument: $required" }
    }
    if ('--workspace' -in $arguments) { throw 'Packaging must not build the acceptance lab' }
    if ($env:CARGO_TARGET_DIR -ne 'preserve-caller-target') { throw 'Caller target changed' }
    & $builder -Configuration debug -TargetDirectory target
    if ('--release' -in $global:ProductBuildCalls[1]) { throw 'Debug build selected release' }
    $global:ProductBuildExit = 27
    $failed = $false
    try { & $builder -Configuration release -TargetDirectory target } catch {
        $failed = $_.Exception.Message -match 'exit code 27'
    }
    if (-not $failed) { throw 'Cargo failure must fail packaging' }
    if ($env:CARGO_TARGET_DIR -ne 'preserve-caller-target') { throw 'Failure changed caller target' }
    if ((Get-Location).Path -ne $originalDirectory) { throw 'Failure changed caller directory' }
    Write-Output 'build-windows-product.tests.ps1: PASS'
} finally {
    $env:CARGO_TARGET_DIR = $originalTarget
    Remove-Item Function:\cargo
    Remove-Variable ProductBuildCalls,ProductBuildExit -Scope Global
}
