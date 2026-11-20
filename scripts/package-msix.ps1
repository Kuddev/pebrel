[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string] $Version,
    [Parameter(Mandatory = $true)][string] $IdentityName,
    [Parameter(Mandatory = $true)][string] $Publisher,
    [Parameter(Mandatory = $true)][string] $PublisherDisplayName,
    [ValidateSet('x64', 'arm64')][string] $Architecture = 'x64',
    [ValidateSet('debug', 'release')][string] $Configuration = 'release',
    [string] $TargetDirectory,
    [string] $OutputDirectory,
    [string] $MakeAppxPath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
. (Join-Path $PSScriptRoot 'windows-msix.ps1')
Assert-PebrelMsixIdentity $IdentityName $Publisher $PublisherDisplayName $Version
if (-not $MakeAppxPath) { $MakeAppxPath = Find-PebrelMakeAppx }
$MakeAppxPath = (Resolve-Path -LiteralPath $MakeAppxPath -ErrorAction Stop).Path
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $repo 'dist' }
if (-not [IO.Path]::IsPathRooted($OutputDirectory)) { $OutputDirectory = Join-Path $repo $OutputDirectory }
$outputRoot = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Path $outputRoot -Force | Out-Null
$output = Join-Path $outputRoot "Pebrel-v$Version-windows-$Architecture.msix"
if (Test-Path -LiteralPath $output) { throw "MSIX already exists: $output" }
$stage = Join-Path $outputRoot ('.msix-stage-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $stage | Out-Null
try {
    # 复用产品构建、新鲜度、版本、架构和完整资源清单，避免维护第二套 payload。
    $zipOutput = Join-Path $stage 'portable'
    & (Join-Path $PSScriptRoot 'package-release.ps1') -Version $Version -Configuration $Configuration `
        -Architecture $Architecture -TargetDirectory $TargetDirectory -OutputDirectory $zipOutput
    $zip = Join-Path $zipOutput "Pebrel-v$Version-windows-$Architecture.zip"
    $layout = Join-Path $stage 'layout'
    Expand-Archive -LiteralPath $zip -DestinationPath $layout
    New-PebrelMsixLayout -Root $layout -IdentityName $IdentityName -Publisher $Publisher `
        -PublisherDisplayName $PublisherDisplayName -Version $Version -Architecture $Architecture
    $temporary = Join-Path $stage 'package.msix'
    & $MakeAppxPath pack /h SHA256 /d $layout /p $temporary
    if ($LASTEXITCODE -ne 0) { throw "MakeAppx validation/packaging failed: $LASTEXITCODE" }
    # 不使用 /nv；只有 SDK 完成语义校验后才发布本地输出文件。
    Move-Item -LiteralPath $temporary -Destination $output
    [PSCustomObject]@{
        Path = $output
        SHA256 = (Get-FileHash -LiteralPath $output -Algorithm SHA256).Hash.ToLowerInvariant()
        IdentityName = $IdentityName
        Publisher = $Publisher
        Version = "$Version.0"
        Architecture = $Architecture
        Signed = $false
        Configuration = $Configuration
    }
} finally {
    # 仅清理此次创建的直接子目录，不根据归档或身份字段拼接递归删除目标。
    $resolved = (Resolve-Path -LiteralPath $stage).Path
    if ([IO.Path]::GetDirectoryName($resolved) -ne $outputRoot -or
        -not [IO.Path]::GetFileName($resolved).StartsWith('.msix-stage-') -or
        ((Get-Item -LiteralPath $resolved).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw "Unexpected MSIX staging location: $resolved"
    }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
