[CmdletBinding()]
param(
    [ValidateSet('debug', 'release')][string] $Configuration = 'release',
    [string] $TargetDirectory
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repo = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
if ([string]::IsNullOrWhiteSpace($TargetDirectory)) { $TargetDirectory = Join-Path $repo 'target' }
if (-not [IO.Path]::IsPathRooted($TargetDirectory)) { $TargetDirectory = Join-Path $repo $TargetDirectory }
$targetRoot = [IO.Path]::GetFullPath($TargetDirectory)
$helper = Join-Path $repo 'tools/windows-context-menu'
$helperTarget = Join-Path $targetRoot 'windows-context-menu'
$buildArgs = @('build', '--locked', '--manifest-path', (Join-Path $helper 'Cargo.toml'), '--target-dir', $helperTarget)
if ($Configuration -eq 'release') { $buildArgs += '--release' }
& cargo @buildArgs
if ($LASTEXITCODE -ne 0) { throw "Explorer extension build failed: $LASTEXITCODE" }
$compiler = & rustc -vV
if ($LASTEXITCODE -ne 0) { throw 'Unable to determine Rust host architecture' }
$architecture = if ($compiler -contains 'host: x86_64-pc-windows-msvc') { 'x64' }
    elseif ($compiler -contains 'host: aarch64-pc-windows-msvc') { 'arm64' }
    else { throw 'Explorer extension requires a native Windows MSVC build' }
$stage = Join-Path $targetRoot "$Configuration/context-menu"
New-Item -ItemType Directory -Path (Join-Path $stage 'assets') -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $helperTarget "$Configuration/pebrel_context_menu.dll") -Destination $stage
Copy-Item -LiteralPath (Join-Path $helper 'package/assets/logo.png') -Destination (Join-Path $stage 'assets/logo.png')
$manifest = [IO.File]::ReadAllText((Join-Path $helper 'package/AppxManifest.xml'))
$manifest = $manifest.Replace('ProcessorArchitecture="x64"', "ProcessorArchitecture=`"$architecture`"")
[IO.File]::WriteAllText((Join-Path $stage 'AppxManifest.xml'), $manifest, [Text.UTF8Encoding]::new($false))
Write-Output "Explorer extension staged at $stage"
