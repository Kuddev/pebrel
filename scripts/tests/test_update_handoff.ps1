# Native helper contracts with small process fixtures. Full product upgrade and
# AI/WSL recovery acceptance must be run separately with actual product packages.
param([string]$OutputRoot)
$ErrorActionPreference = 'Stop'
$OutputEncoding = [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not $OutputRoot) { $OutputRoot = Join-Path $root ('tmp\handoff-tests-' + [guid]::NewGuid()) }
$null = New-Item -ItemType Directory -Path $OutputRoot -Force
$fixture = Join-Path $PSScriptRoot 'fixtures\update_process.cs'
$compiler = Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
$helper = Join-Path $root 'nebula_app\src\update_download\handoff.ps1'
$powershell = Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\powershell.exe'
$old = Join-Path $OutputRoot 'old.exe'
$candidate = Join-Path $OutputRoot 'candidate.exe'
$reinstall = Join-Path $OutputRoot 'reinstall.exe'
$installer = Join-Path $OutputRoot 'installer.exe'
& $compiler /nologo /target:exe "/out:$old" $fixture
if ($LASTEXITCODE) { throw 'Could not build old process fixture' }
& $compiler /nologo /target:exe /define:NEW_VERSION "/out:$candidate" $fixture
if ($LASTEXITCODE) { throw 'Could not build candidate fixture' }
& $compiler /nologo /target:exe /define:SAME_VERSION "/out:$reinstall" $fixture
if ($LASTEXITCODE) { throw 'Could not build same-version candidate fixture' }
& $compiler /nologo /target:exe /define:INSTALLER "/out:$installer" $fixture
if ($LASTEXITCODE) { throw 'Could not build installer fixture' }

function Assert([bool]$Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
function Wait-File([string]$Path, [int]$Seconds = 12) {
    $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
    while (-not (Test-Path -LiteralPath $Path)) {
        if ([DateTime]::UtcNow -ge $deadline) { throw "Timed out waiting for $Path" }
        Start-Sleep -Milliseconds 50
    }
}
function Write-Json([string]$Path, $Value) {
    [System.IO.File]::WriteAllText($Path, ($Value | ConvertTo-Json -Depth 8), [System.Text.UTF8Encoding]::new($false))
}

$results = @()
foreach ($scenario in @('cancel', 'success', 'checksum', 'creation-time', 'installer-failure', 'other-process', 'late-process', 'reinstall', 'repair-identical', 'upgrade-noop', 'helper-unlocked', 'helper-held')) {
    $directory = Join-Path $OutputRoot $scenario
    $installation = Join-Path $directory 'installed app'
    $transaction = Join-Path $directory 'transaction'
    $config = Join-Path $directory 'config'
    $null = New-Item -ItemType Directory -Path $installation, $transaction, $config -Force
    $executable = Join-Path $installation 'pebrel.exe'
    Copy-Item -LiteralPath $old -Destination $executable
    $payload = if ($scenario -eq 'reinstall') { $reinstall } elseif ($scenario -eq 'repair-identical') { $old } else { $candidate }
    Copy-Item -LiteralPath $payload -Destination (Join-Path $directory 'candidate.exe')
    [System.IO.File]::WriteAllText((Join-Path $installation 'unins000.exe'), 'fixture marker')
    $env:PEBREL_HANDOFF_FIXTURE = $directory
    $env:PEBREL_HANDOFF_FAIL_INSTALL = if ($scenario -eq 'installer-failure') { '1' } else { '0' }
    $env:PEBREL_HANDOFF_NOOP_INSTALL = if ($scenario -eq 'upgrade-noop') { '1' } else { '0' }
    $parent = Start-Process -FilePath $executable -ArgumentList 'wait' -PassThru
    $null = $parent.Handle
    $runner = $null
    $other = $null
    $helperLock = $null
    try {
        if ($scenario -in @('helper-unlocked', 'helper-held')) {
            $null = New-Item -ItemType Directory -Path (Join-Path $installation 'runtime') -Force
            $helperPath = Join-Path $installation 'runtime\pebrel-hook.exe'
            [System.IO.File]::WriteAllText($helperPath, 'old helper fixture')
            $helperLock = [System.IO.FileStream]::new($helperPath, [System.IO.FileMode]::Open,
                [System.IO.FileAccess]::Read, [System.IO.FileShare]::Read)
        }
        $plan = @{
            schema = 1; transaction = $scenario; executable = $executable; installation = $installation
            config_directory = $config; installer = $installer
            bytes = (Get-Item -LiteralPath $installer).Length
            sha256 = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash
            version = '9.9.9'; original_version = '1.8.0'; guard_path = (Join-Path $directory 'install.nebula-lock')
            participants = @(@{ pid = $parent.Id; created = $parent.StartTime.ToUniversalTime().ToFileTimeUtc().ToString() })
        }
        if ($scenario -eq 'checksum') { $plan.sha256 = '0' * 64 }
        if ($scenario -in @('reinstall', 'repair-identical')) { $plan.version = '1.8.0' }
        if ($scenario -eq 'creation-time') { $plan.participants[0].created = '1' }
        if ($scenario -eq 'other-process') {
            $other = Start-Process -FilePath $executable -ArgumentList 'wait-other' -PassThru
        }
        $planPath = Join-Path $transaction 'plan.json'
        Write-Json $planPath $plan
        $runner = Start-Process -FilePath $powershell -ArgumentList @(
            '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File',
            ('"' + $helper + '"'), '-PlanPath', ('"' + $planPath + '"')) -PassThru
        $resultPath = Join-Path $transaction 'result.json'
        if ($scenario -in @('checksum', 'creation-time', 'other-process')) {
            Wait-File $resultPath
            Assert (-not $parent.HasExited) 'Rejected plan must leave the original process open'
            Assert (-not (Test-Path (Join-Path $directory 'installer-started'))) 'Rejected plan started setup'
        } else {
            Wait-File (Join-Path $transaction 'ready.json')
            Assert (-not $parent.HasExited) 'Readiness must not stop the original process'
            Assert (-not (Test-Path (Join-Path $directory 'installer-started'))) 'Setup ran before commit'
            $locked = $false
            try {
                $probe = [System.IO.FileStream]::new($plan.guard_path, [System.IO.FileMode]::Open,
                    [System.IO.FileAccess]::ReadWrite, [System.IO.FileShare]::None)
                $probe.Dispose()
            } catch [System.IO.IOException] { $locked = $true }
            Assert $locked 'Ready helper did not own the installation lock'
            if ($scenario -eq 'late-process') {
                $other = Start-Process -FilePath $executable -ArgumentList 'wait-other' -PassThru
            }
            if ($scenario -eq 'cancel') {
                Write-Json (Join-Path $transaction 'cancel.json') @{}
                Wait-File $resultPath
                Assert (-not $parent.HasExited) 'Cancel stopped the original process'
            } else {
                Write-Json (Join-Path $transaction 'workspace.json') @(@{ tabs = @(@{ cwd = 'fixture' }) })
                Write-Json (Join-Path $transaction 'commit.json') @{ transaction = $scenario }
                Start-Sleep -Milliseconds 300
                Assert (-not (Test-Path (Join-Path $directory 'installer-started'))) 'Setup ran while the old process was alive'
                [System.IO.File]::WriteAllText((Join-Path $directory 'exit-parent'), 'exit')
                Assert ($parent.WaitForExit(5000)) 'Fixture parent did not exit'
                if ($scenario -eq 'helper-unlocked') {
                    Start-Sleep -Milliseconds 350
                    Assert (-not (Test-Path (Join-Path $directory 'installer-started'))) 'Setup started with the helper still locked'
                    $helperLock.Dispose()
                    $helperLock = $null
                }
                Wait-File $resultPath
                if ($scenario -in @('success', 'installer-failure', 'reinstall', 'repair-identical', 'upgrade-noop', 'helper-unlocked', 'helper-held')) {
                    Wait-File (Join-Path $directory 'new-launch')
                    $launch = [System.IO.File]::ReadAllLines((Join-Path $directory 'new-launch'))
                    Assert ($launch[0] -eq $executable) 'Relaunch selected another installation'
                    Assert ($launch[1] -eq $planPath) 'Restore ticket was not passed to the new process'
                    Assert ($launch[2] -eq $config) 'Relaunch lost the configuration directory'
                }
                Assert (Test-Path (Join-Path $transaction 'workspace.json')) 'Update discarded the original snapshot'
            }
        }
        Assert ($runner.WaitForExit(5000)) 'Helper did not finish'
        $result = Get-Content -LiteralPath $resultPath -Raw -Encoding UTF8 | ConvertFrom-Json
        Assert ($result.success -eq ($scenario -in @('success', 'reinstall', 'repair-identical', 'helper-unlocked'))) 'Unexpected helper outcome'
        if ($result.success) {
            $expected = (Get-FileHash -LiteralPath $payload -Algorithm SHA256).Hash
            Assert ($result.executable_sha256 -eq $expected) 'Installed binary differs from candidate'
            Assert (Test-Path -LiteralPath (Join-Path $installation 'runtime\pebrel-hook.exe')) 'Successful setup did not repair the helper'
            Assert ([System.IO.File]::ReadAllText((Join-Path $installation 'runtime\pebrel-hook.exe')) -eq 'repaired helper fixture') 'Helper still contains the original payload'
        }
        if ($scenario -in @('other-process', 'late-process')) {
            Assert (-not $other.HasExited) 'Update stopped an unprepared process'
            Assert (-not (Test-Path (Join-Path $directory 'installer-started'))) 'Setup ran with an unprepared process'
        }
        if ($scenario -in @('installer-failure', 'upgrade-noop', 'helper-held')) {
            Assert $result.recovered_original 'Untouched old executable was not recovered'
        }
        if ($scenario -eq 'helper-held') {
            Assert (-not (Test-Path (Join-Path $directory 'installer-started'))) 'A blocked helper allowed a partial installation'
            Assert ($result.error -match 'pebrel-hook.exe') 'The blocked file is missing from the failure details'
        }
        $probe = [System.IO.FileStream]::new($plan.guard_path, [System.IO.FileMode]::OpenOrCreate,
            [System.IO.FileAccess]::ReadWrite, [System.IO.FileShare]::None)
        $probe.Dispose()
        $results += @{ scenario = $scenario; passed = $true }
        Write-Output "$scenario : passed"
    } finally {
        if ($helperLock) { $helperLock.Dispose() }
        if ($runner -and -not $runner.HasExited) { $runner.Kill(); $runner.WaitForExit() }
        if (-not $parent.HasExited) {
            [System.IO.File]::WriteAllText((Join-Path $directory 'exit-parent'), 'exit')
            if (-not $parent.WaitForExit(5000)) { $parent.Kill(); $parent.WaitForExit() }
        }
        if ($other) {
            [System.IO.File]::WriteAllText((Join-Path $directory 'exit-other'), 'exit')
            if (-not $other.WaitForExit(5000)) { $other.Kill(); $other.WaitForExit() }
            $other.Dispose()
        }
        if ($runner) { $runner.Dispose() }
        $parent.Dispose()
    }
}
Write-Json (Join-Path $OutputRoot 'results.json') $results
