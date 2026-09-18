# Runs from the transaction directory, outside the installation being replaced.
# The application grants installation only by writing commit.json after ready.json.
param([Parameter(Mandatory = $true)][string]$PlanPath)
$ErrorActionPreference = 'Stop'
$OutputEncoding = [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$transaction = Split-Path -Parent $PlanPath
$utf8 = [System.Text.UTF8Encoding]::new($false)
$handles = @()
$guard = $null
$installerGuard = $null
$committed = $false
$originalDigest = $null

function Write-State([string]$Name, $Value) {
    $destination = Join-Path $transaction $Name
    $temporary = "$destination.$PID.tmp"
    $bytes = $utf8.GetBytes(($Value | ConvertTo-Json -Depth 12 -Compress))
    $stream = [System.IO.FileStream]::new($temporary, [System.IO.FileMode]::CreateNew,
        [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
    try { $stream.Write($bytes, 0, $bytes.Length); $stream.Flush($true) }
    finally { $stream.Dispose() }
    if (Test-Path -LiteralPath $destination) {
        [System.IO.File]::Replace($temporary, $destination, $null)
    } else { [System.IO.File]::Move($temporary, $destination) }
}

function Same-Path([string]$First, [string]$Second) {
    return [string]::Equals([System.IO.Path]::GetFullPath($First).TrimEnd('\'),
        [System.IO.Path]::GetFullPath($Second).TrimEnd('\'), [StringComparison]::OrdinalIgnoreCase)
}

function Read-Version([string]$Executable) {
    $start = [System.Diagnostics.ProcessStartInfo]::new($Executable, '--version')
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $probe = [System.Diagnostics.Process]::Start($start)
    try {
        $output = $probe.StandardOutput.ReadToEndAsync()
        $errors = $probe.StandardError.ReadToEndAsync()
        if (-not $probe.WaitForExit(10000)) {
            $probe.Kill()
            $probe.WaitForExit()
            throw 'Application version verification timed out'
        }
        if ($probe.ExitCode -ne 0) { throw 'Application version verification failed' }
        return $output.Result.Trim()
    } finally { $probe.Dispose() }
}

function Launch-Workspace {
    $env:PEBREL_UPDATE_RESTORE = $PlanPath
    $env:PEBREL_CONFIG_DIR = $plan.config_directory
    $env:NEBULA_CONFIG_DIR = $plan.config_directory
    Start-Process -FilePath $exe -WorkingDirectory $installation | Out-Null
}

function Check-UnpreparedProcesses {
    foreach ($other in [System.Diagnostics.Process]::GetProcessesByName([System.IO.Path]::GetFileNameWithoutExtension($exe))) {
        try {
            if ((Same-Path $other.MainModule.FileName $exe) -and
                -not (@($plan.participants | Where-Object { [int]$_.pid -eq $other.Id }).Count)) {
                throw 'Another process from this installation is not prepared for update'
            }
        } finally { $other.Dispose() }
    }
}

try {
    if ((Get-Item -LiteralPath $PlanPath).Length -gt 1048576) { throw 'Update plan exceeds limit' }
    $plan = Get-Content -LiteralPath $PlanPath -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($plan.schema -ne 1 -or $plan.transaction -notmatch '^[a-zA-Z0-9-]{1,96}$') {
        throw 'Invalid update plan'
    }
    $exe = [System.IO.Path]::GetFullPath($plan.executable)
    $installation = Split-Path -Parent $exe
    if (-not (Same-Path $installation $plan.installation)) { throw 'Installation path changed' }
    if (Same-Path $transaction $installation) { throw 'Helper cannot run inside installation' }
    if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw 'Installed application is missing' }
    if (-not (Test-Path -LiteralPath (Join-Path $installation 'unins000.exe') -PathType Leaf)) {
        throw 'This directory is not an installer-managed installation'
    }
    if ($plan.version -notmatch '^[0-9]+\.[0-9]+\.[0-9]+([+-][a-zA-Z0-9.-]+)?$' -or
        $plan.original_version -notmatch '^[0-9]+\.[0-9]+\.[0-9]+([+-][a-zA-Z0-9.-]+)?$' -or
        $plan.sha256 -notmatch '^[a-fA-F0-9]{64}$') { throw 'Invalid package identity' }
    if ($plan.participants.Count -lt 1 -or $plan.participants.Count -gt 32) { throw 'Invalid participant count' }
    # Own a kernel-backed lifetime lock before acknowledging readiness. A crash
    # releases it automatically; the empty file is not itself a stale lock.
    $guard = [System.IO.FileStream]::new($plan.guard_path, [System.IO.FileMode]::OpenOrCreate,
        [System.IO.FileAccess]::ReadWrite, [System.IO.FileShare]::None)
    foreach ($participant in $plan.participants) {
        $process = [System.Diagnostics.Process]::GetProcessById([int]$participant.pid)
        $null = $process.Handle # acquire the exact kernel object before checking identity
        if (-not (Same-Path $process.MainModule.FileName $exe)) { throw 'Participant executable differs' }
        if ($process.StartTime.ToUniversalTime().ToFileTimeUtc().ToString() -ne $participant.created) {
            throw 'Participant process identity changed'
        }
        $handles += $process
    }
    Check-UnpreparedProcesses
    # Keep the package open without write sharing from verification through setup.
    $installerGuard = [System.IO.FileStream]::new($plan.installer, [System.IO.FileMode]::Open,
        [System.IO.FileAccess]::Read, [System.IO.FileShare]::Read)
    if ($installerGuard.Length -ne $plan.bytes) { throw 'Installer size changed' }
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try { $actual = [BitConverter]::ToString($sha.ComputeHash($installerGuard)).Replace('-', '') }
    finally { $sha.Dispose() }
    if ($actual -ne $plan.sha256) { throw 'Installer checksum changed' }
    $originalDigest = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash
    if ((Read-Version $exe) -notmatch ('^Pebrel ' + [regex]::Escape($plan.original_version) + '(\s|$)')) {
        throw 'Original application version differs'
    }
    Write-State 'ready.json' @{ transaction = $plan.transaction; helper = $PID }

    $deadline = [DateTime]::UtcNow.AddSeconds(90)
    $commitPath = Join-Path $transaction 'commit.json'
    while (-not (Test-Path -LiteralPath $commitPath)) {
        if ([DateTime]::UtcNow -ge $deadline -or (Test-Path -LiteralPath (Join-Path $transaction 'cancel.json'))) {
            throw 'Installation was not committed'
        }
        if (@($handles | Where-Object { -not $_.HasExited }).Count -eq 0) {
            throw 'Application exited before committing the update'
        }
        Start-Sleep -Milliseconds 100
    }
    $commit = Get-Content -LiteralPath $commitPath -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($commit.transaction -ne $plan.transaction) { throw 'Commit identity differs' }
    $committed = $true
    foreach ($process in $handles) {
        if (-not $process.WaitForExit(60000)) { throw 'Application has not exited; installation aborted' }
    }
    Check-UnpreparedProcesses
    # No Restart Manager process-name shutdown: every participant has already
    # saved and exited. DIR reuses this validated installation without a chooser.
    $setupLog = Join-Path $transaction 'installer.log'
    $arguments = '/SP- /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /NOCLOSEAPPLICATIONS /NORESTARTAPPLICATIONS' +
        ' /DIR="' + $installation + '" /LOG="' + $setupLog + '"'
    $setup = Start-Process -FilePath $plan.installer -ArgumentList $arguments -PassThru
    $setup.WaitForExit()
    if ($setup.ExitCode -ne 0) { throw "Installer failed with exit code $($setup.ExitCode)" }
    $reported = Read-Version $exe
    if ($reported -notmatch ('^Pebrel ' + [regex]::Escape($plan.version) + '(\s|$)')) {
        throw 'Installed application did not report the expected version'
    }
    $installedDigest = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash
    if ($plan.version -eq $plan.original_version -and $installedDigest -eq $originalDigest) {
        throw 'Same-version installation did not replace the application binary'
    }
    Write-State 'result.json' @{
        transaction = $plan.transaction; success = $true; version = $plan.version
        executable_sha256 = $installedDigest
    }
    $guard.Dispose()
    $guard = $null
    Launch-Workspace
} catch {
    $failure = $_.Exception.Message
    # Restart only an unchanged, still executable old binary after all original
    # participants exited. This is recovery, not a claim of installer rollback.
    $recoverOriginal = $false
    if ($committed -and $originalDigest -and
        @($handles | Where-Object { -not $_.HasExited }).Count -eq 0) {
        try {
            Check-UnpreparedProcesses
            $recoverOriginal = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash -eq $originalDigest -and
                (Read-Version $exe) -match ('^Pebrel ' + [regex]::Escape($plan.original_version) + '(\s|$)')
        } catch { $recoverOriginal = $false }
    }
    try { Write-State 'result.json' @{
        transaction = $plan.transaction; success = $false; committed = $committed
        recovered_original = $recoverOriginal; error = $failure
    } }
    catch { [Console]::Error.WriteLine('Could not persist update failure') }
    if ($recoverOriginal) {
        if ($guard) { $guard.Dispose(); $guard = $null }
        try { Launch-Workspace }
        catch { [Console]::Error.WriteLine('Could not restart the original application') }
    }
    exit 1
} finally {
    foreach ($process in $handles) { $process.Dispose() }
    if ($installerGuard) { $installerGuard.Dispose() }
    if ($guard) { $guard.Dispose() }
}
