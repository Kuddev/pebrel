$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot '../windows-package-architecture.ps1')
$fixtureRoot = Join-Path ([IO.Path]::GetTempPath()) ("pebrel-package-architecture-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $fixtureRoot | Out-Null
try {
    foreach ($architecture in @('x64', 'arm64')) {
        $bytes = New-Object byte[] 96
        [BitConverter]::GetBytes([uint16]0x5A4D).CopyTo($bytes, 0)
        [BitConverter]::GetBytes([uint32]64).CopyTo($bytes, 0x3C)
        [BitConverter]::GetBytes([uint32]0x4550).CopyTo($bytes, 64)
        $machine = if ($architecture -eq 'arm64') { 0xAA64 } else { 0x8664 }
        [BitConverter]::GetBytes([uint16]$machine).CopyTo($bytes, 68)
        foreach ($name in @('pebrel.exe', 'pebrel-hook.exe', 'conpty.dll', 'OpenConsole.exe')) {
            [IO.File]::WriteAllBytes((Join-Path $fixtureRoot $name), $bytes)
        }
        Assert-WindowsPackageArchitecture -Root $fixtureRoot -Architecture $architecture
        $wrong = if ($architecture -eq 'arm64') { 'x64' } else { 'arm64' }
        $rejected = $false
        try { Assert-WindowsPackageArchitecture -Root $fixtureRoot -Architecture $wrong } catch {
            $rejected = $_.Exception.Message -match 'architecture does not match'
        }
        if (-not $rejected) { throw 'Mislabeled machine type was accepted' }
        # The application alone is insufficient: the packaged hook and console runtime must match.
        $bytes[68] = 0
        [IO.File]::WriteAllBytes((Join-Path $fixtureRoot 'pebrel-hook.exe'), $bytes)
        $rejected = $false
        try { Assert-WindowsPackageArchitecture -Root $fixtureRoot -Architecture $architecture } catch {
            $rejected = $_.Exception.Message -match 'pebrel-hook.exe'
        }
        if (-not $rejected) { throw 'Mismatched hook helper was accepted' }
        [IO.File]::WriteAllBytes((Join-Path $fixtureRoot 'pebrel.exe'), [byte[]]@(0, 1, 2))
        $rejected = $false
        try { Assert-WindowsPackageArchitecture -Root $fixtureRoot -Architecture $architecture } catch {
            $rejected = $_.Exception.Message -match 'Invalid executable header'
        }
        if (-not $rejected) { throw 'Truncated executable was accepted' }
    }
    Write-Output 'windows-package-architecture.tests.ps1: PASS'
} finally {
    Remove-Item -LiteralPath $fixtureRoot -Recurse -Force
}
