# Work around the windows-11-arm runner image: Windows PowerShell 5.1 there
# renders nothing (not even `Write-Output`) when its PSModulePath is the
# registry value alone, and recovers as soon as the PowerShell 7 module
# directories that the runner's pwsh step shell injects come first. Pebrel
# rebuilds child environments from the registry (the stock console host rule), so
# the injected entries never reach the default tab; putting them into the
# user-level registry value keeps the rebuild faithful and PS 5.1 alive.
# Evidence: environment-block replay and PSModulePath bisect on the ARM64
# runner, 2026-09-22.
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$pwsh7 = @(
    (Join-Path $HOME 'Documents\PowerShell\Modules'),
    (Join-Path $env:ProgramFiles 'PowerShell\Modules'),
    (Join-Path $env:ProgramFiles 'PowerShell\7\Modules')
)
$machine = [Environment]::GetEnvironmentVariable('PSModulePath', 'Machine')
$user = [Environment]::GetEnvironmentVariable('PSModulePath', 'User')
$entries = @($pwsh7) + @(($user, $machine) -join ';' -split ';' | Where-Object { $_ })
$value = ($entries | Select-Object -Unique) -join ';'
[Environment]::SetEnvironmentVariable('PSModulePath', $value, 'User')
"User PSModulePath: $value"
