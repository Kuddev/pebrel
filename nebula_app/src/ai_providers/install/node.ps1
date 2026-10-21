# 用户级 Node 仅在 npm 缺失或 Node 过旧时安装；不需要管理员权限。
function Add-InstallPath([string]$directory) {
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (($userPath -split ';') -notcontains $directory) {
        [Environment]::SetEnvironmentVariable('Path', "$directory;$userPath", 'User')
    }
    $env:Path = "$directory;$env:Path"
}
$nodeReady = $false
if ((Get-Command node -ErrorAction SilentlyContinue) -and (Get-Command npm.cmd -ErrorAction SilentlyContinue)) {
    $nodeReady = [version]((& node --version).TrimStart('v')) -ge [version]'22.19.0'
}
if (-not $nodeReady) {
    $arch = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64' -or $env:PROCESSOR_ARCHITEW6432 -eq 'ARM64') { 'arm64' } else { 'x64' }
    $base = 'https://nodejs.org/dist/latest-v22.x'
    $sums = (Invoke-WebRequest -UseBasicParsing "$base/SHASUMS256.txt").Content
    $match = [regex]::Match($sums, "(?m)^([a-f0-9]{64})\s+(node-v22\.\d+\.\d+-win-$arch\.zip)\r?$")
    if (-not $match.Success) { throw 'Node release checksum is missing' }
    $archive = $match.Groups[2].Value
    $stage = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid().ToString())
    New-Item -ItemType Directory -Path $stage | Out-Null
    try {
        Write-Host 'Downloading Node.js 22 LTS...'
        $zip = Join-Path $stage $archive
        Invoke-WebRequest -UseBasicParsing "$base/$archive" -OutFile $zip
        if ((Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash -ine $match.Groups[1].Value) { throw 'Node download checksum mismatch' }
        $root = Join-Path $env:LOCALAPPDATA 'Pebrel\cli-node'
        Expand-Archive -LiteralPath $zip -DestinationPath $root -Force
        Add-InstallPath (Join-Path $root ([IO.Path]::GetFileNameWithoutExtension($archive)))
    } finally {
        # stage 是本次创建的随机临时目录，不使用用户输入参与删除路径。
        Remove-Item -LiteralPath $stage -Recurse -Force
    }
}
