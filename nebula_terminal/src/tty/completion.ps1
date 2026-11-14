# Query the native editor without changing its buffer or prediction settings.
if (Get-Command Set-PSReadLineKeyHandler -ErrorAction SilentlyContinue) {
    try {
        # Legacy ConPTY does not translate xterm F24; the modified F12 sequence
        # works with both native console and VT input, including the in-box host.
        $query = Get-Command Get-PSReadLineKeyHandler -ErrorAction Stop
        $existing = if ($query.Parameters.ContainsKey('Chord')) {
            Get-PSReadLineKeyHandler -Chord Ctrl+Shift+F12 -ErrorAction SilentlyContinue
        } else {
            # Windows PowerShell's bundled PSReadLine lacks -Chord.
            Get-PSReadLineKeyHandler | Where-Object { $_.Key -in 'Ctrl+Shift+F12','Shift+Ctrl+F12' }
        }
        if (-not $existing -or $existing.Function -eq 'Unbound' -or
            ($global:PebrelEditorHandlerName -and $existing.Function -eq $global:PebrelEditorHandlerName)) {
            $global:PebrelEditorHandlerName = 'PebrelNativeEditor:' + $global:PebrelShellToken
            Set-PSReadLineKeyHandler -Chord Ctrl+Shift+F12 -ScriptBlock {
                param($key, $arg)
                if ($null -ne $global:PebrelPointerArgument) {
                    Complete-PebrelPointerArgument
                    return
                }
                $line = ''
                $cursor = 0
                [Microsoft.PowerShell.PSConsoleReadLine]::GetBufferState([ref]$line, [ref]$cursor)
                if ($line.Length -le 4096) {
                    $owner = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($global:PebrelShellToken))
                    $payload = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($owner + "`nutf16`n" + $cursor + "`n" + $line))
                    [Console]::Write("$([char]27)]1337;SetUserVar=pebrel_editor=$payload$([char]7)")
                }
            } -BriefDescription $global:PebrelEditorHandlerName
            # The prompt-redraw pairing: an owned chord re-bound to the native
            # ClearScreen, probed separately so a user-held chord leaves query,
            # completion and pointer support untouched — the redraw capability
            # is simply not advertised, and the terminal never falls back to a
            # keyboard byte that invokes user rebinding. Ctrl+Shift+F9 keeps
            # Ctrl+Shift+F11 free for clipboard.ps1's native paste binding.
            # Identity is the binding we installed, not the Function display:
            # PSReadLine reports a scriptblock's BriefDescription as Function,
            # so -BriefDescription ClearScreen can imitate the native name and
            # Function alone proves nothing. The Description token below is
            # what installation and the per-prompt re-check match; anything
            # else already on the chord is user-owned and stays untouched.
            $clearExisting = if ($query.Parameters.ContainsKey('Chord')) {
                Get-PSReadLineKeyHandler -Chord Ctrl+Shift+F9 -ErrorAction SilentlyContinue
            } else {
                @(Get-PSReadLineKeyHandler | Where-Object { $_.Key -in 'Ctrl+Shift+F9','Shift+Ctrl+F9' })
            }
            if (-not $clearExisting -or $clearExisting.Function -eq 'Unbound') {
                # The redraw install is an optional capability: a PSReadLine that
                # refuses the binding must not take the editor and pointer init
                # down with it, so its failure stays inside this try and only
                # leaves the redraw advertisement off.
                try {
                    $global:PebrelRedrawDescription = 'PebrelRedraw:' + $global:PebrelShellToken
                    # -Function and -Description live in different parameter sets
                    # (PSReadLine 2.4.5: "Parameter 'Function' cannot be specified
                    # in parameter set 'ScriptBlock.'"), so the ScriptBlock set is
                    # the only one that carries the identity token. The handler
                    # performs the native ClearScreen work itself — erase display
                    # and scrollback, then re-prompt — exactly what the built-in
                    # does, and Description stays what the per-prompt re-check
                    # matches; Function is a display name scriptblocks imitate.
                    Set-PSReadLineKeyHandler -Chord Ctrl+Shift+F9 -ScriptBlock {
                        param($key, $arg)
                        [Console]::Write("$([char]27)[2J$([char]27)[3J$([char]27)[H")
                        [Microsoft.PowerShell.PSConsoleReadLine]::InvokePrompt()
                    } -Description $global:PebrelRedrawDescription
                    $global:PebrelRedrawReady = $true
                } catch { $global:PebrelRedrawReady = $false }
            }
            # PSReadLine 2.0 loses surrogate pairs inserted after native editing.
            # Advertise that boundary so completion cannot silently alter a name.
            $owner = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($global:PebrelShellToken))
            $capability = if ((Get-Module PSReadLine).Version -lt [Version]'2.1.0') { 'basic' } else { 'unicode' }
            $global:PebrelEditorReady = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($owner + "`n" + $capability))
            $global:PebrelCompletionInputReady = $true
        }
    } catch { $global:PebrelCompletionInputReady = $false }
}

# Prompt-redraw re-verification shared by both bootstraps. Reuses the same
# lookup fallback as the installation for PSReadLine builds without -Chord.
function global:Test-PebrelRedrawBinding {
    try {
        if (-not $global:PebrelRedrawReady) { return $false }
        $query = Get-Command Get-PSReadLineKeyHandler -ErrorAction Stop
        $bound = if ($query.Parameters.ContainsKey('Chord')) {
            Get-PSReadLineKeyHandler -Chord Ctrl+Shift+F9 -ErrorAction SilentlyContinue
        } else {
            @(Get-PSReadLineKeyHandler -ErrorAction Stop | Where-Object { $_.Key -in 'Ctrl+Shift+F9','Shift+Ctrl+F9' })
        }
        # Function is only a display description (see the installation probe),
        # and the installed binding is a scriptblock carrying its identity in
        # Description: the token is what proves the chord still carries the
        # binding this session installed.
        $bound -and $bound.Description -eq $global:PebrelRedrawDescription
    } catch { $false }
}
