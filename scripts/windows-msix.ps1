# MSIX 清单与静态资源准备；构建和发布仍由调用方持有，不加载或执行插件。
function Assert-PebrelMsixIdentity {
    param([string] $IdentityName, [string] $Publisher, [string] $PublisherDisplayName, [string] $Version)
    if ($IdentityName -cnotmatch '^[A-Za-z0-9.-]{3,50}$' -or $IdentityName -match '^(Microsoft|Windows)\.') {
        throw 'Use the exact Package/Identity/Name reserved in Partner Center (3-50 ASCII letters, numbers, dots or hyphens).'
    }
    if ([string]::IsNullOrWhiteSpace($Publisher) -or $Publisher -notmatch '(^|,)\s*CN=') {
        throw 'Publisher must be the X.500 identity from Partner Center, including CN=.'
    }
    $null = [System.Security.Cryptography.X509Certificates.X500DistinguishedName]::new($Publisher)
    if ([string]::IsNullOrWhiteSpace($PublisherDisplayName) -or $PublisherDisplayName.Length -gt 256) {
        throw 'PublisherDisplayName must contain 1-256 characters.'
    }
    if ($Version -notmatch '^[0-9]+\.[0-9]+\.[0-9]+$') { throw 'MSIX requires a stable major.minor.patch version.' }
    $parts = @($Version.Split('.') | ForEach-Object { [int] $_ })
    if ($parts[0] -lt 1 -or @($parts | Where-Object { $_ -gt 65535 }).Count -gt 0) {
        throw 'MSIX version components must fit UInt16, with a nonzero major version.'
    }
}

function New-PebrelMsixLayout {
    param(
        [Parameter(Mandatory = $true)][string] $Root,
        [Parameter(Mandatory = $true)][string] $IdentityName,
        [Parameter(Mandatory = $true)][string] $Publisher,
        [Parameter(Mandatory = $true)][string] $PublisherDisplayName,
        [Parameter(Mandatory = $true)][string] $Version,
        [ValidateSet('x64', 'arm64')][string] $Architecture = 'x64'
    )
    Assert-PebrelMsixIdentity $IdentityName $Publisher $PublisherDisplayName $Version
    $repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
    if (-not (Test-Path -LiteralPath $Root -PathType Container)) { throw "Missing MSIX layout: $Root" }
    . (Join-Path $PSScriptRoot 'windows-package-architecture.ps1')
    Assert-WindowsPackageArchitecture -Root $Root -RuntimeDirectory (Join-Path $Root 'runtime') -Architecture $Architecture
    $template = [IO.File]::ReadAllText((Join-Path $repo 'packaging/windows/AppxManifest.xml'), [Text.Encoding]::UTF8)
    $values = @{
        IDENTITY_NAME = $IdentityName
        PUBLISHER = $Publisher
        PUBLISHER_DISPLAY_NAME = $PublisherDisplayName
        VERSION = "$Version.0"
        ARCHITECTURE = $Architecture
    }
    # MatchEvaluator 防止用户字段中的 @TOKEN@ 被第二次替换；XML 值始终转义。
    $manifest = [regex]::Replace($template, '@([A-Z_]+)@', [Text.RegularExpressions.MatchEvaluator] {
        param($match)
        $key = $match.Groups[1].Value
        if (-not $values.ContainsKey($key)) { throw "Unknown manifest token: $key" }
        [Security.SecurityElement]::Escape($values[$key])
    })
    $null = [xml] $manifest
    $manifestPath = Join-Path $Root 'AppxManifest.xml'
    [IO.File]::WriteAllText($manifestPath, $manifest, [Text.UTF8Encoding]::new($false))

    # 缩放现有产品图标，不另造品牌素材；System.Drawing 仅在 Windows 打包进程内使用。
    Add-Type -AssemblyName System.Drawing
    $assets = Join-Path $Root 'Assets'
    New-Item -ItemType Directory -Path $assets -Force | Out-Null
    $source = [Drawing.Image]::FromFile((Join-Path $repo 'extra/logo/nebula-titanium.png'))
    try {
        foreach ($entry in @{'StoreLogo.png'=50; 'Square44x44Logo.png'=44; 'Square150x150Logo.png'=150}.GetEnumerator()) {
            $bitmap = [Drawing.Bitmap]::new([int]$entry.Value, [int]$entry.Value)
            $graphics = [Drawing.Graphics]::FromImage($bitmap)
            try {
                $graphics.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
                $graphics.DrawImage($source, 0, 0, [int]$entry.Value, [int]$entry.Value)
                $bitmap.Save((Join-Path $assets $entry.Key), [Drawing.Imaging.ImageFormat]::Png)
            } finally { $graphics.Dispose(); $bitmap.Dispose() }
        }
    } finally { $source.Dispose() }
}

function Find-PebrelMakeAppx {
    $command = Get-Command makeappx.exe -ErrorAction SilentlyContinue
    if ($command) { return $command.Source }
    $kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10/bin'
    $sdk = Get-ChildItem -LiteralPath $kits -Directory -ErrorAction Stop |
        Where-Object { $_.Name -match '^10\.0\.[0-9]+\.0$' } |
        Sort-Object { [version] $_.Name } -Descending
    foreach ($version in $sdk) {
        $candidate = Join-Path $version.FullName 'x64/makeappx.exe'
        if (Test-Path -LiteralPath $candidate -PathType Leaf) { return $candidate }
    }
    throw 'Install the Windows 10/11 SDK packaging tools, or supply -MakeAppxPath.'
}
