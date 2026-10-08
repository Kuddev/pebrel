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
        if (-not $existing -or $existing.Function -eq 'Unbound') {
            Set-PSReadLineKeyHandler -Chord Ctrl+Shift+F12 -ScriptBlock {
                param($key, $arg)
                $line = ''
                $cursor = 0
                [Microsoft.PowerShell.PSConsoleReadLine]::GetBufferState([ref]$line, [ref]$cursor)
                if ($line.Length -le 4096) {
                    $owner = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($global:PebrelShellToken))
                    $payload = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($owner + "`nutf16`n" + $cursor + "`n" + $line))
                    [Console]::Write("$([char]27)]1337;SetUserVar=pebrel_editor=$payload$([char]7)")
                }
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
