# One-click launcher for the adjacent portable preview. It never selects an installed instance.
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$pairingMutex = $null
$ownsPairingMutex = $false

function Read-PreviewEndpoint {
    $file = Join-Path $PSScriptRoot 'preview-profile\runtime.port'
    if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { return $null }
    $value = [System.IO.File]::ReadAllText($file).Trim()
    if ($value -notmatch '^([0-9]{1,5})\s+([a-fA-F0-9]{32})(?:\s+1)?$') { return $null }
    $port = [int]$Matches[1]
    $token = $Matches[2]
    if ($port -lt 1 -or $port -gt 65535) { return $null }
    $client = [System.Net.Sockets.TcpClient]::new()
    try {
        if (-not $client.ConnectAsync('127.0.0.1', $port).Wait(1000)) { return $null }
        $stream = $client.GetStream()
        $stream.ReadTimeout = 1000
        $stream.WriteTimeout = 1000
        $bytes = [System.Text.Encoding]::UTF8.GetBytes("PING $token`n")
        $stream.Write($bytes, 0, $bytes.Length)
        $reply = [byte[]]::new(3)
        $received = 0
        while ($received -lt $reply.Length) {
            $count = $stream.Read($reply, $received, $reply.Length - $received)
            if ($count -eq 0) { return $null }
            $received += $count
        }
        if ($reply[0] -eq 79 -and $reply[1] -eq 75 -and $reply[2] -eq 10) { return "$port $token" }
    } catch { return $null }
    finally { $client.Dispose() }
    return $null
}

try {
    Write-Host 'Pebrel - Connect phone / 连接手机'
    $hasher = [System.Security.Cryptography.SHA256]::Create()
    try { $key = [BitConverter]::ToString($hasher.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($PSScriptRoot.ToUpperInvariant()))).Replace('-', '') }
    finally { $hasher.Dispose() }
    $pairingMutex = [System.Threading.Mutex]::new($false, "Local\PebrelPhonePreview_$key")
    try { $ownsPairingMutex = $pairingMutex.WaitOne(0) }
    catch [System.Threading.AbandonedMutexException] { $ownsPairingMutex = $true }
    if (-not $ownsPairingMutex) {
        Add-Type -AssemblyName System.Windows.Forms
        [System.Windows.Forms.MessageBox]::Show('连接手机工具已打开，请使用已有的配对窗口。 / The pairing helper is already open. Use its existing window.', 'Pebrel') | Out-Null
        exit 0
    }
    $node = Get-Command node.exe -ErrorAction SilentlyContinue
    $npm = Get-Command npm.cmd -ErrorAction SilentlyContinue
    if (-not $node -or -not $npm) {
        Write-Host '首次使用需要安装 Node.js 22 或更新版本，然后重新双击连接手机。'
        Write-Host 'Install Node.js 22 or newer, then open Connect-Phone.cmd again: https://nodejs.org/'
        exit 1
    }
    $nodeVersion = & $node.Source --version
    if ($LASTEXITCODE -ne 0 -or $nodeVersion -notmatch '^v(\d+)\.' -or [int]$Matches[1] -lt 22) {
        Write-Host '请更新至 Node.js 22 或更新版本。 / Node.js 22 or newer is required.'
        exit 1
    }
    $desktop = Join-Path $PSScriptRoot 'pebrel.exe'
    $kit = Join-Path $PSScriptRoot 'mobile\relay'
    if (-not (Test-Path -LiteralPath $desktop) -or -not (Test-Path -LiteralPath (Join-Path $kit 'pairing.mjs'))) {
        Write-Host '请将完整便携包解压后再打开连接手机。 / Extract the complete portable archive first.'
        exit 1
    }
    $env:PEBREL_CONFIG_DIR = Join-Path $PSScriptRoot 'preview-profile'
    Remove-Item Env:\PEBREL_RUNTIME_ENDPOINT -ErrorAction SilentlyContinue
    Remove-Item Env:\NEBULA_RUNTIME_ENDPOINT -ErrorAction SilentlyContinue
    $endpoint = Read-PreviewEndpoint
    if (-not $endpoint) {
        Write-Host '正在打开 Pebrel… / Opening Pebrel…'
        Start-Process -FilePath $desktop -WorkingDirectory $PSScriptRoot | Out-Null
        for ($attempt = 0; $attempt -lt 30; $attempt++) {
            Start-Sleep -Milliseconds 500
            $endpoint = Read-PreviewEndpoint
            if ($endpoint) { break }
        }
    }
    if (-not $endpoint) {
        Write-Host '未能连接此目录的 Pebrel。请以普通用户启动预览版后重试。'
        Write-Host 'Could not reach this preview. Open it without administrator privileges, then retry.'
        exit 1
    }
    # Child-only environment: no token is put in arguments, logs or the QR invitation.
    $env:PEBREL_RUNTIME_ENDPOINT = $endpoint
    Set-Location -LiteralPath $kit
    $stamp = Join-Path $kit 'node_modules\.pebrel-package-lock'
    $digest = (Get-FileHash -LiteralPath 'package-lock.json' -Algorithm SHA256).Hash
    if (-not (Test-Path -LiteralPath $stamp) -or [System.IO.File]::ReadAllText($stamp).Trim() -ne $digest) {
        Write-Host '首次使用正在准备连接工具… / Preparing pairing tools for first use…'
        & $npm.Source ci --omit=dev --ignore-scripts --no-audit --no-fund
        if ($LASTEXITCODE -ne 0) {
            Write-Host '准备失败，请检查网络后重新打开。 / Setup failed. Check the network and open again.'
            exit 1
        }
        [System.IO.File]::WriteAllText($stamp, $digest)
    }
    Write-Host '即将打开二维码。保持此窗口运行，手机扫码即可连接；关闭此窗口会断开手机。'
    Write-Host 'Opening the QR page. Keep this window running; closing it disconnects the phone.'
    & $node.Source pairing.mjs --lan --config private/lan-desktop.json --name $env:COMPUTERNAME --port 8765 --allow-input
    exit $LASTEXITCODE
} catch {
    Write-Host '连接工具未能启动。请确认已完整解压、Node.js 可用且此目录可写，然后重试。'
    Write-Host 'Could not start pairing. Check the complete extraction, Node.js and folder permissions, then retry.'
    exit 1
} finally {
    Remove-Item Env:\PEBREL_RUNTIME_ENDPOINT -ErrorAction SilentlyContinue
    if ($ownsPairingMutex) { $pairingMutex.ReleaseMutex() }
    if ($pairingMutex) { $pairingMutex.Dispose() }
}
