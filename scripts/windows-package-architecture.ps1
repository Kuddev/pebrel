# Validate the machine type of every executable payload before naming a Windows package.
function Assert-WindowsPackageArchitecture {
    param(
        [Parameter(Mandatory = $true)][string] $Root,
        [ValidateSet('x64', 'arm64')][string] $Architecture = 'x64'
    )
    $machine = if ($Architecture -eq 'arm64') { 0xAA64 } else { 0x8664 }
    foreach ($name in @('pebrel.exe', 'pebrel-hook.exe', 'conpty.dll', 'OpenConsole.exe')) {
        $path = Join-Path $Root $name
        $stream = [IO.File]::OpenRead($path)
        $reader = [IO.BinaryReader]::new($stream)
        try {
            if ($stream.Length -lt 64 -or $reader.ReadUInt16() -ne 0x5A4D) {
                throw "Invalid executable header: $path"
            }
            $stream.Position = 0x3C
            $offset = $reader.ReadUInt32()
            if ($offset -lt 64 -or $offset -gt $stream.Length - 6) {
                throw "Invalid executable header offset: $path"
            }
            $stream.Position = $offset
            if ($reader.ReadUInt32() -ne 0x4550 -or $reader.ReadUInt16() -ne $machine) {
                throw "Executable architecture does not match $Architecture`: $path"
            }
        } finally {
            $reader.Dispose()
            $stream.Dispose()
        }
    }
}
