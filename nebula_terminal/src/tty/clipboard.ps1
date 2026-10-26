# PSReadLine's native Paste inserts one edit without invoking per-character keys.
# Keep an existing user binding; advertise only while this editor can handle it.
# Handler names are user-controlled, so a plain "Paste" does not prove ownership.
$global:PebrelClipboardHandlerName = 'PebrelClipboardPaste:' + $global:PebrelShellToken
function global:Get-PebrelClipboardBinding {
    $query = Get-Command Get-PSReadLineKeyHandler -ErrorAction Stop
    if ($query.Parameters.ContainsKey('Chord')) {
        Get-PSReadLineKeyHandler -Chord Ctrl+Shift+F11 -ErrorAction Stop
    } else {
        Get-PSReadLineKeyHandler | Where-Object { $_.Key -in 'Ctrl+Shift+F11','Shift+Ctrl+F11' }
    }
}

try {
    if ((Get-Command Set-PSReadLineKeyHandler -ErrorAction SilentlyContinue) -and
        (Get-PSReadLineOption -ErrorAction Stop).EditMode -ne 'Vi') {
        $clipboardBinding = Get-PebrelClipboardBinding
        if (-not $clipboardBinding -or $clipboardBinding.Function -eq 'Unbound') {
            # Register the native delegate directly. A ScriptBlock invokes a
            # PowerShell pipeline and can change Unicode console rendering.
            $pebrelNativePaste = [Microsoft.PowerShell.PSConsoleReadLine].GetMethod('Paste').CreateDelegate(
                [Action[Nullable[ConsoleKeyInfo],object]])
            [Microsoft.PowerShell.PSConsoleReadLine]::SetKeyHandler(
                [string[]]@('Ctrl+Shift+F11'), $pebrelNativePaste,
                $global:PebrelClipboardHandlerName, 'Paste the clipboard with PSReadLine')
        }
    }
} catch {}

function global:Get-PebrelClipboardCapability {
    try {
        # Vi can change its active map without rendering another shell prompt.
        if ((Get-PSReadLineOption -ErrorAction Stop).EditMode -ne 'Vi' -and
            (Get-PebrelClipboardBinding).Function -eq $global:PebrelClipboardHandlerName) {
            return $global:PebrelShellToken
        }
    } catch {}
    return ''
}
