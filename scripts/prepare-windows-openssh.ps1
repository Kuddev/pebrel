[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string] $Destination,
    [string] $ArchivePath,
    [ValidateSet('x64', 'arm64')][string] $Architecture = 'x64'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$OutputEncoding = [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)

# Microsoft Win32-OpenSSH（MIT）的固定版本。Pebrel 用它起一个只监听回环、
# 只信任本次会话临时密钥的 sshd 实例（设计与理由见
# architecture/notes/nebula_app/remote_claude/2026-10-08-remote-claude-over-ssh.md）。
#
# 9.5+ 起 sshd 拆成主程序 + 会话/认证辅助进程，只带 sshd.exe 会起不来：
# 下面每一行都是运行时要加载的整套文件，多一个不装、少一个不跑。
$release = '10.0.0.0p2-Preview'
if ($Architecture -eq 'arm64') {
    $archiveHash = '698C6AEC31C1DD0FB996206E8741F4531A97355686B5431EF347D531B07FCD42'
    $archiveName = 'pebrel-openssh-source-arm64-10.0.0.0p2.zip'
    $sourceUrl = "https://github.com/PowerShell/Win32-OpenSSH/releases/download/$release/OpenSSH-ARM64.zip"
    $entryPrefix = 'OpenSSH-ARM64/'
    $machine = 0xAA64
    $expected = [ordered]@{
        'libcrypto.dll' = '7AD2B7721893C54AD6E4FEC1A3477701FB48975323C2C4AC6CD0B8C972AB242A'
        'moduli' = '089B524DBF38C6520E96BF4DEE11DA93F0328E6E89FD10EBC8C9A317B81D50B7'
        'scp.exe' = '53115DE3294C52A3A2CC8B87A29FF71DAA42CD72C6783DED3F12EAC8E92662C1'
        'sftp-server.exe' = 'F3A2F3B27094EC12518EAFDCC39D1CDFD6F1550EAC5C24850314A1A776783B8B'
        'sftp.exe' = '5E5560D2ACB920E84F15762680BE8542104AD58C508CE1D1ECCA43408B26274D'
        'ssh-keygen.exe' = 'C94940E4EA52FB073E460532884E0C14202E631DB1D488A3A681D8230D57E0D6'
        'ssh-shellhost.exe' = 'D89F9A420268120789CF9DC1B94EF4313DA1A258C3C57C172EDA2FE999DED7E4'
        'ssh.exe' = 'FD87CCDFBD8BE33D22B67FA3F1C94BB2327A3E7D24355BF7FD2485D356F1976D'
        'sshd-auth.exe' = '0C7CA28ED3649EF6F0CE1B3698CC7A92F51B86286FC077D5513F1BF5E48B178F'
        'sshd-session.exe' = '9F368188F703BD39594ABEFC29F28DF5E97664ACB937A1D26288B500ECE4D463'
        'sshd.exe' = 'F3EB3230D454DC662D5C5F09EFACE5ACDEB2B196CCDD41B562644433A1562906'
        'sshd_config_default' = '796F518D1F4BB03D775DA710C259B518EB770D123133DBD6429CA7E14D3C224B'
        'LICENSE.txt' = '568C41F330A50A7D2D2EBA9D3F7807CAAAF4E091705AB6652EC26A3F9D1ACFDB'
        'NOTICE.txt' = '2CD5F5D0064BC909DFE0F0FC8F4787882C7C379F2730D89E5FAA0C2BC57C620B'
    }
}
else {
    $archiveHash = '23F50F3458C4C5D0B12217C6A5DDFDE0137210A30FA870E98B29827F7B43ABA5'
    $archiveName = 'pebrel-openssh-source-x64-10.0.0.0p2.zip'
    $sourceUrl = "https://github.com/PowerShell/Win32-OpenSSH/releases/download/$release/OpenSSH-Win64.zip"
    $entryPrefix = 'OpenSSH-Win64/'
    $machine = 0x8664
    $expected = [ordered]@{
        'libcrypto.dll' = '4652E861C0335EE80A51306CEAB75AA35C8865B235F97CE7DD5A0FD9DAB44B5D'
        'moduli' = '089B524DBF38C6520E96BF4DEE11DA93F0328E6E89FD10EBC8C9A317B81D50B7'
        'scp.exe' = 'CA014DDB0A3C058719E7061EB604DE7DFE1F7732954792AC8176E6C1FC99B4A3'
        'sftp-server.exe' = '63462C6904943F5F32CA363065F2EB7E883EE308F40C7C1637A307A253522151'
        'sftp.exe' = '97271EA46FA2EEB5E9E22CD5E919931727A2A11F79993C1EF151B4F618D9FD22'
        'ssh-keygen.exe' = 'B51FDD26BE0F7C83398D18E5354A0ACB0406A9DE25516791758FE63BBE3AE870'
        'ssh-shellhost.exe' = 'F090CB45E3B9DD830201993FC274E75A9BE2058C1D4E900E85EFF334DFD5A84C'
        'ssh.exe' = '6890C128C86CC2C38AAD9FCB32A82B851FF3D38C714A1F656B8E445D7CD5E1C6'
        'sshd-auth.exe' = '71B11418681E1AE8A3EB84494658F4CA66A7DAC7CCC7674F3C74A36A3A5B7D14'
        'sshd-session.exe' = '5B28AD2046596454BD1E0B1AB19E8D66945DEF1D18EC9BEC6F0EF538600A03CF'
        'sshd.exe' = 'D66150486D472B8748CAE2D6F5078A210DAE91621447974BDE028F077A4EF90C'
        'sshd_config_default' = '796F518D1F4BB03D775DA710C259B518EB770D123133DBD6429CA7E14D3C224B'
        'LICENSE.txt' = '568C41F330A50A7D2D2EBA9D3F7807CAAAF4E091705AB6652EC26A3F9D1ACFDB'
        'NOTICE.txt' = '2CD5F5D0064BC909DFE0F0FC8F4787882C7C379F2730D89E5FAA0C2BC57C620B'
    }
}

function Assert-RuntimeMachine([string] $Path) {
    if ([IO.Path]::GetExtension($Path) -notin @('.exe', '.dll')) { return }
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
        Invoke-WebRequest -Uri $sourceUrl -OutFile $ArchivePath -UseBasicParsing -TimeoutSec 300
    }
}
if ((Get-FileHash -LiteralPath $ArchivePath -Algorithm SHA256).Hash -ne $archiveHash) {
    throw 'The pinned OpenSSH source archive failed SHA256 verification.'
}

Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [System.IO.Compression.ZipFile]::OpenRead((Resolve-Path $ArchivePath).Path)
$temporaries = [System.Collections.Generic.List[string]]::new()
try {
    New-Item -ItemType Directory -Path $Destination -Force | Out-Null
    foreach ($name in $expected.Keys) {
        $target = Join-Path $Destination $name
        if ((Test-Path -LiteralPath $target -PathType Leaf) -and
            (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash -eq $expected[$name]) {
            Assert-RuntimeMachine $target
            continue
        }
        $entry = $archive.GetEntry("$entryPrefix$name")
        if ($null -eq $entry) { throw "OpenSSH archive is missing $entryPrefix$name" }
        $temporary = "$target.$([guid]::NewGuid().ToString('N')).tmp"
        $temporaries.Add($temporary)
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
        }
        finally { Remove-Item -LiteralPath $temporary -Force -ErrorAction SilentlyContinue }
    }
}
finally {
    $archive.Dispose()
    # 校验失败时不留半个运行时；已发布的目标文件不受影响。
    foreach ($temporary in $temporaries) {
        Remove-Item -LiteralPath $temporary -Force -ErrorAction SilentlyContinue
    }
}

foreach ($name in $expected.Keys) {
    Write-Output "$name SHA256 $($expected[$name])"
}
