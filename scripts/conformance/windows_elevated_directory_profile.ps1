param(
    [Parameter(Mandatory)][string]$Executable,
    [Parameter(Mandatory)][ValidateSet('powershell.exe', 'pwsh.exe')][string]$ExpectedHandover
)
$ErrorActionPreference = 'Stop'
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if (!$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'The directory handover fixture requires an already elevated Windows runner.'
}
$root = Join-Path $env:RUNNER_TEMP ("handover-" + [guid]::NewGuid().ToString('n'))
$config = New-Item -Type Directory (Join-Path $root 'profile') -Force
$first = New-Item -Type Directory (Join-Path $root 'first folder')
$second = New-Item -Type Directory (Join-Path $root 'second folder')
$env:PEBREL_CONFIG_DIR = $config.FullName
$env:NEBULA_CONFIG_DIR = $config.FullName
$shell = (Get-Command pwsh.exe).Source
$profileData = @{ version = 1; profiles = @(@{
    id = 'fixture-pwsh'; name = 'Imported PowerShell'; shell_id = 'pwsh'
    command = $shell; args = @('-NoLogo')
}) }
$profileData | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8NoBOM (Join-Path $config 'terminal_profiles.json')
@('shell=profile:pwsh|fixture-pwsh', 'windowing_behavior=use_any_existing', 'restore_session=0',
  'tray=0', 'ai_hooks=0', 'auto_check_updates=0') | Set-Content -Encoding utf8NoBOM (Join-Path $config 'pebrel_settings.txt')

function Wait-Shell([int]$OwnerId, [int[]]$Existing, [string]$Expected) {
    $limit = [DateTime]::UtcNow.AddSeconds(45)
    do {
        $rows = @(Get-CimInstance -ClassName Win32_Process -Property ProcessId,ParentProcessId,Name,ExecutablePath)
        $parents = @($OwnerId)
        for ($level = 0; $level -lt 6 -and $parents.Length; $level++) {
            $children = @($rows | Where-Object { $parents -contains $_.ParentProcessId })
            foreach ($child in $children) {
                if (($child.Name -in @('pwsh.exe','powershell.exe')) -and ($Existing -notcontains $child.ProcessId)) {
                    if ($child.Name -ne $Expected) { throw "Expected $Expected, found $($child.Name) below the resident." }
                    if ($Expected -eq 'pwsh.exe' -and $child.ExecutablePath -ne $shell) { throw "Unexpected PowerShell 7 path." }
                    return $child.ProcessId
                }
            }
            $parents = @($children | ForEach-Object { $_.ProcessId })
        }
        Start-Sleep -Milliseconds 300
    } while ([DateTime]::UtcNow -lt $limit)
    throw "No additional $Expected process started below the resident."
}

$owner = Start-Process -FilePath $Executable -PassThru -ArgumentList ('--gpui --working-directory "' + $first.FullName + '"')
try {
    $initial = Wait-Shell $owner.Id @() 'pwsh.exe'
    $handover = Start-Process -FilePath $Executable -PassThru -ArgumentList ('--gpui --working-directory "' + $second.FullName + '"')
    try {
        if (!$handover.WaitForExit(30000) -or $handover.ExitCode -ne 0) { throw 'Directory handover did not return successfully.' }
        $additional = Wait-Shell $owner.Id @($initial) $ExpectedHandover
        Write-Output "Resident $($owner.Id) kept the initial PowerShell 7 process $initial and started $ExpectedHandover ($additional) for the directory handover."
    } finally {
        if (!$handover.HasExited) { $handover.Kill($true) }
    }
} finally {
    if (!$owner.HasExited) { $owner.Kill($true); $owner.WaitForExit() }
}
