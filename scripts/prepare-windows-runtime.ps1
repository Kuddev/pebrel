[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string] $Destination,
    [string] $ArchivePath,
    [ValidateSet('x64', 'arm64')][string] $Architecture = 'x64'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$OutputEncoding = [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)

# Reuse the Microsoft 1.22 runtime pair already shipped with Pebrel's predecessor.
# Only these two redistributables are extracted, never the older application.
$archiveHash = '9B2413144C0434E29749CCBD5C2B0F93E930DAFD22634DD70D34B763037E0DA4'
$sourceUrl = 'https://github.com/Kuddev/pebrel/releases/download/v1.5.0/NebulaTerminal-v1.5.0-windows-x64.zip'
$expected = [ordered]@{
    'conpty.dll' = '375BFB0479B6C53836AB307E3F9FD17BEDBD733F2E9690943D0F12E72FB80777'
    'OpenConsole.exe' = '55B18996761C88C351820E82508E05AB0EC2194AEEAD20724FDFAEEDEC076EF4'
}
$entries = @{
    'conpty.dll' = 'runtime\conpty.dll'
    'OpenConsole.exe' = 'runtime\OpenConsole.exe'
}
$archiveName = 'pebrel-conpty-source-v1.5.0.zip'
$machine = 0x8664
if ($Architecture -eq 'arm64') {
    # Microsoft MIT-licensed redistributables; the NuGet declares build 17763+.
    # Keep the previously shipped x64 runtime unchanged.
    $sourceUrl = 'https://api.nuget.org/v3-flatcontainer/microsoft.windows.console.conpty/1.24.260710001/microsoft.windows.console.conpty.1.24.260710001.nupkg'
    $archiveName = 'pebrel-conpty-source-1.24.260710001.nupkg'
    $archiveHash = '175640566A3B59C4B132070EE96C2C77E5AB7EDD2E92732A5EB3610BBF63D90E'
    $expected = [ordered]@{
        'conpty.dll' = 'DB3D173640B172BAFD42D5B541B638A9AEEC1C7D0E40DD636BF02822A32C912C'
        'OpenConsole.exe' = 'ED7622FD0D3BEDC9AB9F122F5E58EDF0DEF9E7999224F52DD395BA9F54EDBE09'
    }
    $entries = @{
        'conpty.dll' = 'runtimes/win-arm64/native/conpty.dll'
        'OpenConsole.exe' = 'build/native/runtimes/arm64/OpenConsole.exe'
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
