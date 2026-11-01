# Native PSReadLine edits use owned key chords; all editing stays in PSReadLine.
function global:Test-PebrelPointerBinding {
    try {
        if (-not $global:PebrelCompletionInputReady -or
            (Get-Module PSReadLine).Version -lt [Version]'1.2.0' -or
            (Get-PSReadLineOption).EditMode -eq 'Vi') { return $false }
        $bindings = @(Get-PSReadLineKeyHandler -ErrorAction Stop)
        # The editor can retain child chords after a user replaces their prefix.
        # Such children are no longer reachable through the native dispatcher.
        if ($bindings | Where-Object { $_.Key -in 'Ctrl+Shift+F10','Shift+Ctrl+F10' }) { return $false }
        $query = @($bindings | Where-Object { $_.Key -in 'Ctrl+Shift+F12','Shift+Ctrl+F12' })
        if ($query.Count -ne 1 -or $query[0].Function -ne $global:PebrelEditorHandlerName) { return $false }
        foreach ($digit in 0..9) {
            $binding = @($bindings | Where-Object { $_.Key -in "Ctrl+Shift+F10,$digit","Shift+Ctrl+F10,$digit" })
            if ($binding.Count -ne 1 -or $binding[0].Function -ne $global:PebrelPointerHandlerName) { return $false }
        }
        return $true
    } catch { return $false }
}

function global:Send-PebrelPointerValue {
    param([string]$Name, [string]$Value)
    $encoded = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($Value))
    [Console]::Write("$([char]27)]1337;SetUserVar=$Name=$encoded$([char]7)")
}

function global:Start-PebrelPointerReadLine {
    param([switch]$DeferInputBoundary)
    $global:PebrelPointerSnapshot = $null
    $global:PebrelPointerArgument = $null
    if (Test-PebrelPointerBinding) {
        [Console]::Write("$([char]27)]1337;SetUserVar=pebrel_pointer_ready=$global:PebrelShellToken$([char]7)")
        # The console host has already rendered the complete native prompt.
        if (-not $DeferInputBoundary) { [Console]::Write("$([char]27)]133;B$([char]7)") }
    }
}

function global:ConvertTo-PebrelUtf16Offset {
    param([string]$Line, [int]$Scalars)
    $offset = 0
    for ($count = 0; $count -lt $Scalars; $count++) {
        if ($offset -ge $Line.Length) { return -1 }
        if ([char]::IsHighSurrogate($Line[$offset])) { $offset++ }
        $offset++
    }
    return $offset
}

function global:Invoke-PebrelPointerAction {
    param($Argument)
    if ($Argument -isnot [int] -or $Argument -lt 0 -or $Argument -gt 999999 -or
        ($Argument -gt 12291 -and $Argument -lt 20000)) { return }
    $owner = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($global:PebrelShellToken))
    $line = ''; $cursor = 0
    [Microsoft.PowerShell.PSConsoleReadLine]::GetBufferState([ref]$line, [ref]$cursor)
    if ($Argument -eq 0 -or $Argument -ge 20000) {
        $global:PebrelPointerSnapshot = $null
        $unit = if ($Argument -eq 0) { 'utf8' } else { "utf8:$Argument" }
        try {
            $utf8 = New-Object Text.UTF8Encoding($false, $true)
            if (-not (Test-PebrelPointerBinding) -or $utf8.GetByteCount($line) -gt 4096) { throw 'Unsupported editor' }
            # In-box PSReadLine has native cursor/delete APIs too. Its old
            # renderer can lose surrogate pairs after an edit, so reject only
            # affected buffers rather than disabling all ASCII/CJK pointer edits.
            if ((Get-Module PSReadLine).Version -lt [Version]'2.1.0' -and
                $line -match '[\uD800-\uDFFF]') { throw 'Unsupported Unicode buffer' }
            $bytes = $utf8.GetByteCount($line.Substring(0, $cursor))
            $global:PebrelPointerSnapshot = @{ Line=$line; Cursor=$cursor; Token=$global:PebrelShellToken; Anchor=$null }
            Send-PebrelPointerValue 'pebrel_pointer' "$owner`n$unit`n$bytes`n$line"
        } catch {
            Send-PebrelPointerValue 'pebrel_pointer' "$owner`n$($unit.Replace('utf8','unsupported'))"
        }
        return
    }
    $snapshot = $global:PebrelPointerSnapshot
    if (-not $snapshot -or $snapshot.Token -cne $global:PebrelShellToken -or
        $snapshot.Line -cne $line -or $snapshot.Cursor -ne $cursor -or -not (Test-PebrelPointerBinding)) {
        $global:PebrelPointerSnapshot = $null
        Send-PebrelPointerValue 'pebrel_pointer_status' "$owner`nrejected"
        return
    }
    $target = ConvertTo-PebrelUtf16Offset $line (($Argument - 1) % 4097)
    if ($target -lt 0) { return }
    if ($Argument -le 4097) {
        [Microsoft.PowerShell.PSConsoleReadLine]::SetCursorPosition($target)
        $snapshot.Cursor = $target
    } elseif ($Argument -le 8194) {
        $snapshot.Anchor = $target
    } elseif ($null -ne $snapshot.Anchor) {
        $first = [Math]::Min($snapshot.Anchor, $target)
        $last = [Math]::Max($snapshot.Anchor, $target)
        # One native mutation retains the editor's undo record.
        [Microsoft.PowerShell.PSConsoleReadLine]::Delete($first, $last - $first)
        $global:PebrelPointerSnapshot = $null
    }
}

# .NET Unix ReadKey exposes Escape+digit as two keys, whereas ConPTY exposes
# Alt+digit. Use the same public two-key chord contract on both platforms.
# Each fragment is a native callback, not a raw stdin reader or a mirrored draft.
function global:Add-PebrelPointerDigit {
    param($Key)
    $line = ''; $cursor = 0
    [Microsoft.PowerShell.PSConsoleReadLine]::GetBufferState([ref]$line, [ref]$cursor)
    $now = [Diagnostics.Stopwatch]::GetTimestamp()
    $argument = $global:PebrelPointerArgument
    if (-not $argument) {
        $argument = @{ Digits=''; Line=$line; Cursor=$cursor; Started=$now; Rejected=$false }
        $global:PebrelPointerArgument = $argument
    }
    if ($argument.Line -cne $line -or $argument.Cursor -ne $cursor -or
        ($now - $argument.Started) -gt [Diagnostics.Stopwatch]::Frequency -or
        $argument.Digits.Length -ge 6 -or $Key.KeyChar -lt '0' -or $Key.KeyChar -gt '9') {
        $argument.Rejected = $true
    } else {
        $argument.Digits += [string]$Key.KeyChar
    }
}

function global:Complete-PebrelPointerArgument {
    $argument = $global:PebrelPointerArgument
    $global:PebrelPointerArgument = $null
    $line = ''; $cursor = 0
    [Microsoft.PowerShell.PSConsoleReadLine]::GetBufferState([ref]$line, [ref]$cursor)
    if ($argument -and -not $argument.Rejected -and $argument.Line -ceq $line -and
        $argument.Cursor -eq $cursor -and
        ([Diagnostics.Stopwatch]::GetTimestamp() - $argument.Started) -le [Diagnostics.Stopwatch]::Frequency) {
        Invoke-PebrelPointerAction ([int]$argument.Digits)
    } else {
        $global:PebrelPointerSnapshot = $null
    }
}

try {
    $name = 'PebrelNativePointer:' + $global:PebrelShellToken
    $bindings = @(Get-PSReadLineKeyHandler -ErrorAction Stop | Where-Object {
        $_.Key -match '^(Ctrl\+Shift|Shift\+Ctrl)\+F10(,|$)'
    })
    # Never replace the user's prefix or any chord beneath it.
    if (-not ($bindings | Where-Object { $_.Function -ne $name })) {
        $global:PebrelPointerHandlerName = $name
        foreach ($digit in 0..9) {
            Set-PSReadLineKeyHandler -Chord "Ctrl+Shift+F10,$digit" -ScriptBlock {
                param($key, $arg)
                Add-PebrelPointerDigit $key
            } -BriefDescription $name
        }
    }
} catch {}
