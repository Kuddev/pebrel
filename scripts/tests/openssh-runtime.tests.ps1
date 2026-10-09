[CmdletBinding()]
param(
    [string] $ArchivePath,
    [ValidateSet('x64', 'arm64')][string] $Architecture = 'x64'
)

$ErrorActionPreference = 'Stop'
$prepare = Join-Path $PSScriptRoot '../prepare-windows-openssh.ps1'
$root = Join-Path ([IO.Path]::GetTempPath()) "pebrel-openssh-test-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $root | Out-Null
try {
    $destination = Join-Path $root 'runtime'
    $listed = @(& $prepare -Destination $destination -ArchivePath $ArchivePath -Architecture $Architecture)
    $files = @(Get-ChildItem -LiteralPath $destination -File | Sort-Object Name)
    if ($files.Count -ne $listed.Count) {
        throw "Prepared $($files.Count) files but the pin list has $($listed.Count)."
    }
    foreach ($required in @('sshd.exe', 'sshd-session.exe', 'sshd-auth.exe', 'ssh-shellhost.exe', 'sftp-server.exe', 'libcrypto.dll', 'moduli')) {
        if (-not (Test-Path -LiteralPath (Join-Path $destination $required) -PathType Leaf)) {
            throw "Split sshd runtime is missing $required."
        }
    }

    # 已校验的文件不重复替换：时间戳必须保持原样。
    $before = @($files | ForEach-Object { $_.LastWriteTimeUtc.Ticks })
    & $prepare -Destination $destination -ArchivePath $ArchivePath -Architecture $Architecture | Out-Null
    $after = @(Get-ChildItem -LiteralPath $destination -File | Sort-Object Name |
        ForEach-Object { $_.LastWriteTimeUtc.Ticks })
    if (@(Compare-Object $before $after).Count -ne 0) {
        throw 'Verified OpenSSH files were unnecessarily replaced.'
    }

    # 被改坏的已安装文件必须按固定哈希修复。
    $sshd = Join-Path $destination 'sshd.exe'
    $goodHash = (Get-FileHash -LiteralPath $sshd -Algorithm SHA256).Hash
    [IO.File]::WriteAllBytes($sshd, [byte[]]@(0, 1, 2, 3))
    & $prepare -Destination $destination -ArchivePath $ArchivePath -Architecture $Architecture | Out-Null
    if ((Get-FileHash -LiteralPath $sshd -Algorithm SHA256).Hash -ne $goodHash) {
        throw 'A corrupt installed sshd.exe was reused.'
    }

    # 换了整包（错误架构或伪造来源）必须在解包前失败，且不建目标目录。
    $invalid = Join-Path $root 'different.zip'
    [IO.File]::WriteAllBytes($invalid, [byte[]]@(80, 75, 0, 0))
    $rejected = $false
    try { & $prepare -Destination (Join-Path $root 'rejected') -ArchivePath $invalid -Architecture $Architecture }
    catch {
        if ($_.Exception.Message -notlike '*SHA256 verification*') { throw }
        $rejected = $true
    }
    if (-not $rejected) { throw 'A different OpenSSH archive was accepted.' }
    if (Test-Path (Join-Path $root 'rejected')) { throw 'Files were created before archive verification.' }

    if ($ArchivePath) {
        # 显式给了归档时，架构不能再被参数改口：x64 的包不能装成 arm64。
        $otherArchitecture = if ($Architecture -eq 'arm64') { 'x64' } else { 'arm64' }
        $wrongArchitectureRejected = $false
        try {
            & $prepare -Destination (Join-Path $root 'wrong-arch') -ArchivePath $ArchivePath `
                -Architecture $otherArchitecture
        }
        catch {
            if ($_.Exception.Message -notlike '*SHA256 verification*') { throw }
            $wrongArchitectureRejected = $true
        }
        if (-not $wrongArchitectureRejected) {
            throw 'An archive built for the other architecture was accepted.'
        }
    }
    Write-Output "openssh-runtime.tests.ps1: PASS ($Architecture pinned split sshd, reuse, repair, rejected archives)"
}
finally { Remove-Item -LiteralPath $root -Recurse -Force }
