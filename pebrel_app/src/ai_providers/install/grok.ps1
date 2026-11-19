# 官方仓库的 Windows 原生发行包，使用同一 checksums.txt 验证。
if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64' -or $env:PROCESSOR_ARCHITEW6432 -eq 'ARM64') {
    throw 'Grok CLI does not publish a native Windows ARM64 binary.'
}
$base = 'https://github.com/superagent-ai/grok-cli/releases/latest/download'
$stage = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $stage | Out-Null
try {
    Write-Host 'Downloading Grok CLI...'
    $binary = Join-Path $stage 'grok-windows-x64.exe'
    Invoke-WebRequest -UseBasicParsing "$base/grok-windows-x64.exe" -OutFile $binary
    $checksums = (Invoke-WebRequest -UseBasicParsing "$base/checksums.txt").Content
    $match = [regex]::Match($checksums, '(?m)^([a-f0-9]{64})\s+\*?grok-windows-x64\.exe\r?$')
    if (-not $match.Success -or (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash -ine $match.Groups[1].Value) { throw 'Grok download checksum mismatch' }
    $bin = Join-Path $env:USERPROFILE '.grok\bin'
    New-Item -ItemType Directory -Force -Path $bin | Out-Null
    Copy-Item -LiteralPath $binary -Destination (Join-Path $bin 'grok.exe') -Force
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (($userPath -split ';') -notcontains $bin) { [Environment]::SetEnvironmentVariable('Path', "$bin;$userPath", 'User') }
    & (Join-Path $bin 'grok.exe') --version
    if ($LASTEXITCODE -ne 0) { throw 'Grok version verification failed' }
    Write-Host 'Grok installed. Open a new terminal to use grok.'
} finally {
    Remove-Item -LiteralPath $stage -Recurse -Force
}
