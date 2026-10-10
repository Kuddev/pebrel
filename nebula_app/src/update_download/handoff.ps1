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
$payloadGuards = @()
$replaced = @()
$portable = $false

Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class PebrelUpdateProcessImage {
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern bool QueryFullProcessImageName(IntPtr process, uint flags, StringBuilder image, ref uint size);
}
'@

function Process-Image($Process) {
    $image = [Text.StringBuilder]::new(32768)
    $size = [uint32]32768
    if (-not [PebrelUpdateProcessImage]::QueryFullProcessImageName($Process.Handle, 0, $image, [ref]$size)) {
        throw [ComponentModel.Win32Exception]::new([Runtime.InteropServices.Marshal]::GetLastWin32Error())
    }
    return $image.ToString()
}

function Read-Digest([string]$Path) {
    # Get-FileHash is supplied by a module script, which may not be discoverable
    # when the GUI inherited PSModulePath from a different PowerShell edition.
    $stream = [IO.FileStream]::new($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    $sha = [Security.Cryptography.SHA256]::Create()
    try { return [BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-', '') }
    finally { $sha.Dispose(); $stream.Dispose() }
}

function Payload-Path([string]$Base, [string]$Relative) {
    if ($Relative.Length -gt 240 -or $Relative -match '[\\:<>"|?*\x00-\x1f]' -or
        $Relative -match '(^|/)(\.|\.\.|[^/]*[. ])(/|$)' -or
        $Relative -match '(^|/)(CON|PRN|AUX|NUL|CONIN\$|CONOUT\$|COM[1-9]|LPT[1-9])(\.|/|$)' -or
        $Relative -match '^\.pebrel-update' -or $Relative -in @('unins000.exe', 'pebrel-distribution')) {
        throw 'Unsafe portable payload path'
    }
    # Spaces within names are valid; traversal, ADS and reparse points are not.
    $parts = $Relative.Split('/')
    $path = $Base
    if (((Get-Item -LiteralPath $Base -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw 'Portable directory is a reparse point'
    }
    foreach ($part in $parts) {
        if (-not $part) { throw 'Empty portable path component' }
        $path = Join-Path $path $part
        if (Test-Path -LiteralPath $path) {
            $item = Get-Item -LiteralPath $path -Force
            if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw 'Portable path contains a reparse point'
            }
        }
    }
    $full = [IO.Path]::GetFullPath($path)
    if (-not $full.StartsWith(([IO.Path]::GetFullPath($Base).TrimEnd('\') + '\'), [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Portable path escaped its directory'
    }
    return $full
}

function Prepare-Portable {
    if (-not (Same-Path $plan.portable.directory (Join-Path $transaction 'portable'))) {
        throw 'Portable staging directory changed'
    }
    $names = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    $expanded = [long]0
    if ($plan.portable.files.Count -lt 1 -or $plan.portable.files.Count -gt 256) {
        throw 'Invalid portable file count'
    }
    foreach ($entry in $plan.portable.files) {
        if (-not $names.Add($entry.path) -or $entry.sha256 -notmatch '^[a-fA-F0-9]{64}$' -or
            $entry.bytes -lt 0 -or $entry.bytes -gt 536870912) { throw 'Invalid portable file identity' }
        $expanded += [long]$entry.bytes
        if ($expanded -gt 1073741824) { throw 'Portable expanded size exceeds limit' }
        $source = Payload-Path $plan.portable.directory $entry.path
        $null = Payload-Path $installation $entry.path
        $stream = [IO.FileStream]::new($source, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
        $script:payloadGuards += $stream
        if ($stream.Length -ne $entry.bytes) { throw 'Portable file size changed' }
        $sha = [Security.Cryptography.SHA256]::Create()
        try { $hash = [BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-', '') }
        finally { $sha.Dispose() }
        if ($hash -ne $entry.sha256) { throw 'Portable file checksum changed' }
    }
    if (-not $names.Contains('pebrel.exe')) { throw 'Portable executable is missing' }
}

function Replace-Portable {
    $backup = Join-Path $installation ('.pebrel-update-' + $plan.transaction)
    if (Test-Path -LiteralPath $backup) { throw 'Portable backup already exists' }
    $null = [IO.Directory]::CreateDirectory($backup)
    # Back up every affected old file before changing any of them. An occupied
    # file or directory fails with the old package intact.
    foreach ($entry in $plan.portable.files) {
        $target = Payload-Path $installation $entry.path
        if (Test-Path -LiteralPath $target) {
            $probe = [IO.FileStream]::new($target, [IO.FileMode]::Open,
                [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
            $probe.Dispose()
            $saved = Payload-Path $backup $entry.path
            $null = [IO.Directory]::CreateDirectory((Split-Path -Parent $saved))
            [IO.File]::Copy($target, $saved, $false)
        }
    }
    foreach ($entry in $plan.portable.files) {
        $target = Payload-Path $installation $entry.path
        $source = Payload-Path $plan.portable.directory $entry.path
        $saved = Payload-Path $backup $entry.path
        $null = [IO.Directory]::CreateDirectory((Split-Path -Parent $target))
        # Record before copying, so even a failed or partial copy is restored.
        $script:replaced += @{ target = $target; backup = $saved; existed = [IO.File]::Exists($saved) }
        Write-State 'portable-journal.json' $script:replaced
        [IO.File]::Copy($source, $target, $true)
        if ((Read-Digest $target) -ne $entry.sha256) {
            throw 'Replaced portable file checksum differs'
        }
    }
}

function Restore-Portable {
    $errors = @()
    for ($index = $replaced.Count - 1; $index -ge 0; $index--) {
        $entry = $replaced[$index]
        try {
            if ($entry.existed) { [IO.File]::Copy($entry.backup, $entry.target, $true) }
            else { [IO.File]::Delete($entry.target) }
        } catch { $errors += $_.Exception.Message }
    }
    if ($errors.Count) { throw ('Portable rollback incomplete: ' + ($errors -join '; ')) }
}

function Write-State([string]$Name, $Value) {
    $destination = Join-Path $transaction $Name
    $temporary = "$destination.$PID.tmp"
    $bytes = $utf8.GetBytes(($Value | ConvertTo-Json -Depth 12 -Compress))
    $stream = [System.IO.FileStream]::new($temporary, [System.IO.FileMode]::CreateNew,
        [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
    try { $stream.Write($bytes, 0, $bytes.Length); $stream.Flush($true) }
    finally { $stream.Dispose() }
    if (Test-Path -LiteralPath $destination) {
        # Windows PowerShell 5.1 binds a null string argument as an empty path.
        $previous = "$destination.previous"
        [System.IO.File]::Replace($temporary, $destination, $previous)
        [System.IO.File]::Delete($previous)
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
            try { $candidate = Process-Image $other }
            catch {
                # A CLI from another installation can finish between enumeration
                # and module lookup. Only confirmed exits may be ignored.
                if ($other.HasExited) { continue }
                throw
            }
            if ($other.HasExited) { continue }
            if (-not $candidate) { throw 'Could not identify another running application process' }
            if ((Same-Path $candidate $exe) -and
                -not (@($plan.participants | Where-Object { [int]$_.pid -eq $other.Id }).Count)) {
                throw 'Another process from this installation is not prepared for update'
            }
        } finally { $other.Dispose() }
    }
}

function Wait-RuntimeHelperFiles {
    # New helpers have a bounded lifetime, but can still be draining as the
    # application exits. Check before setup copies any file. Old stuck helpers
    # remain a visible failure; never terminate processes by their image name.
    $deadline = [DateTime]::UtcNow.AddSeconds(5)
    $helpers = @('runtime\pebrel-hook.exe', 'pebrel-hook.exe',
        'runtime\nebula-hook.exe', 'nebula-hook.exe')
    while ($true) {
        $busy = $null
        $busyPath = $null
        foreach ($relative in $helpers) {
            $path = Join-Path $installation $relative
            if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { continue }
            try {
                $probe = [System.IO.FileStream]::new($path, [System.IO.FileMode]::Open,
                    [System.IO.FileAccess]::ReadWrite, [System.IO.FileShare]::None)
                $probe.Dispose()
            } catch { $busy = $_.Exception; $busyPath = $path }
        }
        if (-not $busy) { return }
        if ([DateTime]::UtcNow -ge $deadline) {
            throw "Update file is still occupied or not writable: $busyPath. Close programs using it or check directory permissions, then retry."
        }
        Start-Sleep -Milliseconds 100
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
    if (Test-Path -LiteralPath (Join-Path $installation 'pebrel-distribution')) {
        throw 'This copy is managed by an external distribution source'
    }
    $portable = $null -ne $plan.portable
    $managed = Test-Path -LiteralPath (Join-Path $installation 'unins000.exe') -PathType Leaf
    if ($portable -and $managed) { throw 'Portable update cannot replace an installer-managed copy' }
    if (-not $portable -and -not $managed) {
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
        if (-not (Same-Path (Process-Image $process) $exe)) { throw 'Participant executable differs' }
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
    if ($portable) { Prepare-Portable }
    $originalDigest = Read-Digest $exe
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
    Wait-RuntimeHelperFiles
    if ($portable) {
        Replace-Portable
    } else {
        $setupLog = Join-Path $transaction 'installer.log'
        $arguments = '/SP- /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /NOCLOSEAPPLICATIONS /NORESTARTAPPLICATIONS' +
            ' /DIR="' + $installation + '" /LOG="' + $setupLog + '"'
        $setup = Start-Process -FilePath $plan.installer -ArgumentList $arguments -PassThru -WindowStyle Hidden
        $setup.WaitForExit()
        if ($setup.ExitCode -ne 0) { throw "Installer failed with exit code $($setup.ExitCode)" }
    }
    $reported = Read-Version $exe
    if ($reported -notmatch ('^Pebrel ' + [regex]::Escape($plan.version) + '(\s|$)')) {
        throw 'Installed application did not report the expected version'
    }
    $installedDigest = Read-Digest $exe
    # A repair can replace a previously locked helper while leaving pebrel.exe
    # byte-identical. Setup success plus the expected version is authoritative;
    # a changed main-executable hash is not a same-version success requirement.
    Write-State 'result.json' @{
        transaction = $plan.transaction; success = $true; version = $plan.version
        executable_sha256 = $installedDigest
    }
    $guard.Dispose()
    $guard = $null
    Launch-Workspace
} catch {
    $failure = $_.Exception.Message
    $rollbackComplete = $true
    if ($portable -and $replaced.Count) {
        try { Restore-Portable }
        catch { $failure += '; ' + $_.Exception.Message; $rollbackComplete = $false }
    }
    # Restart only an unchanged, still executable old binary after all original
    # participants exited. This is recovery, not a claim of installer rollback.
    $recoverOriginal = $false
    if ($rollbackComplete -and $committed -and $originalDigest -and
        @($handles | Where-Object { -not $_.HasExited }).Count -eq 0) {
        try {
            Check-UnpreparedProcesses
            $recoverOriginal = (Read-Digest $exe) -eq $originalDigest -and
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
    foreach ($stream in $payloadGuards) { $stream.Dispose() }
    if ($guard) { $guard.Dispose() }
}
