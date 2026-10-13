[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string] $Destination,
    [string] $ArchivePath,
    [ValidateSet('x64', 'arm64')][string] $Architecture = 'x64'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$OutputEncoding = [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)

# Use the official stable pair for both architectures so x64 no longer ships the old host.
# Microsoft MIT-licensed redistributables; the NuGet declares build 17763+.
$archiveHash = '175640566A3B59C4B132070EE96C2C77E5AB7EDD2E92732A5EB3610BBF63D90E'
$sourceUrl = 'https://api.nuget.org/v3-flatcontainer/microsoft.windows.console.conpty/1.24.260710001/microsoft.windows.console.conpty.1.24.260710001.nupkg'
$archiveName = 'pebrel-conpty-source-1.24.260710001.nupkg'
$expected = [ordered]@{
    'conpty.dll' = '39FBA2713E2495117B1591AE8C32A3B904BEA7AA66069CF7815E2844C76D75D8'
    'OpenConsole.exe' = 'B7FD936C2668B87B9ECF7B3366DC6568AFC1C6F981874CBA3E955A1C35CF8160'
}
$entries = @{
    'conpty.dll' = "runtimes/win-$Architecture/native/conpty.dll"
    'OpenConsole.exe' = "build/native/runtimes/$Architecture/OpenConsole.exe"
}
$machine = 0x8664
if ($Architecture -eq 'arm64') {
    $expected = [ordered]@{
        'conpty.dll' = 'DB3D173640B172BAFD42D5B541B638A9AEEC1C7D0E40DD636BF02822A32C912C'
        'OpenConsole.exe' = 'ED7622FD0D3BEDC9AB9F122F5E58EDF0DEF9E7999224F52DD395BA9F54EDBE09'
    }
    $machine = 0xAA64
}

function Assert-RuntimeMachine([string] $Path) {
    $bytes = [IO.File]::ReadAllBytes($Path)
    if ($bytes.Length -lt 64 -or $bytes[0] -ne 0x4D -or $bytes[1] -ne 0x5A) {
        throw "Invalid PE runtime: $Path"
    }
    $offset = [BitConverter]::ToInt32($bytes, 0x3C)
    if ($offset -lt 64 -or $offset -gt $bytes.Length - 6 -or
        [BitConverter]::ToUInt32($bytes, $offset) -ne 0x4550 -or
        [BitConverter]::ToUInt16($bytes, $offset + 4) -ne $machine) {
        throw "Runtime PE architecture does not match $Architecture`: $Path"
    }
}

if ([string]::IsNullOrWhiteSpace($ArchivePath)) {
    $ArchivePath = Join-Path ([System.IO.Path]::GetTempPath()) $archiveName
    if (-not (Test-Path -LiteralPath $ArchivePath -PathType Leaf)) {
        Invoke-WebRequest -Uri $sourceUrl -OutFile $ArchivePath -UseBasicParsing -TimeoutSec 120
    }
}
if ((Get-FileHash -LiteralPath $ArchivePath -Algorithm SHA256).Hash -ne $archiveHash) {
    throw 'The pinned runtime source archive failed SHA256 verification.'
}

Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [System.IO.Compression.ZipFile]::OpenRead((Resolve-Path $ArchivePath).Path)
try {
    New-Item -ItemType Directory -Path $Destination -Force | Out-Null
    foreach ($name in $expected.Keys) {
        $target = Join-Path $Destination $name
        if ((Test-Path -LiteralPath $target -PathType Leaf) -and
            (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash -eq $expected[$name]) {
            Assert-RuntimeMachine $target
            continue
        }
        $entry = $archive.GetEntry($entries[$name])
        if ($null -eq $entry) { throw "Runtime archive is missing $($entries[$name])" }
        $temporary = "$target.$([guid]::NewGuid().ToString('N')).tmp"
        try {
            $inputStream = $entry.Open()
            try {
                $outputStream = [System.IO.File]::Create($temporary)
                try { $inputStream.CopyTo($outputStream) } finally { $outputStream.Dispose() }
            } finally { $inputStream.Dispose() }
            if ((Get-FileHash -LiteralPath $temporary -Algorithm SHA256).Hash -ne $expected[$name]) {
                throw "The pinned $name failed SHA256 verification."
            }
            Assert-RuntimeMachine $temporary
            Move-Item -LiteralPath $temporary -Destination $target -Force
        } finally {
            if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary }
        }
    }
} finally { $archive.Dispose() }

foreach ($name in $expected.Keys) {
    Write-Output "$name SHA256 $($expected[$name])"
}
