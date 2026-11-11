# Unix ConsoleHost bootstrap: keep the user's profile, prompt and native editor.
if (-not (Get-Module PSReadLine)) { Import-Module PSReadLine -ErrorAction Stop }
if (-not $global:PebrelShellToken) {
    $global:PebrelShellToken = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes(
        "pwsh:$PID`:$([Guid]::NewGuid().ToString('N'))"))
}
. "$PSScriptRoot/native-editor.ps1"

if (-not $global:PebrelUnixReadLineInstalled) {
    $global:PebrelUnixPreviousPrompt = (Get-Command prompt).ScriptBlock
    $global:PebrelUnixPreviousReadLine = (Get-Command PSConsoleHostReadLine).ScriptBlock
    $global:PebrelUnixReadLineInstalled = $true

    function global:Write-PebrelUnixPrompt {
        param($Succeeded, $ExitCode, $Rendered)
        $code = if ($Succeeded) { 0 } elseif ($ExitCode) { $ExitCode } else { 1 }
        [Console]::Write("$([char]27)]133;D;$code$([char]7)")
        [Console]::Write("$([char]27)]1337;SetUserVar=pebrel_shell=$global:PebrelShellToken$([char]7)")
        if ($global:PebrelCompletionInputReady) {
            [Console]::Write("$([char]27)]1337;SetUserVar=pebrel_editor_ready=$global:PebrelEditorReady$([char]7)")
        }
        # OSC 7 is the native location, including paths with spaces or Unicode.
        if ($PWD.Provider.Name -eq 'FileSystem') {
            $uri = [Uri]::new($PWD.Path).AbsoluteUri
            [Console]::Write("$([char]27)]7;$uri$([char]7)")
        }
        # PSReadLine invokes prompt again on ClearScreen without entering
        # PSConsoleHostReadLine. Re-advertise and mark the returned prompt then too.
        Start-PebrelPointerReadLine -DeferInputBoundary
        "$([char]27)]133;A$([char]7)$Rendered$([char]27)]133;B$([char]7)"
    }
    function global:prompt {
        # Evaluate the original prompt before running reporting commands, so it
        # can still inspect the command's status and render its own appearance.
        Write-PebrelUnixPrompt $? $global:LASTEXITCODE (& $global:PebrelUnixPreviousPrompt)
    }
    function global:PSConsoleHostReadLine {
        $line = & $global:PebrelUnixPreviousReadLine
        if (-not [string]::IsNullOrWhiteSpace($line)) {
            $owner = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($global:PebrelShellToken))
            Send-PebrelPointerValue 'pebrel_input' "$owner`n$line"
            Send-PebrelPointerValue 'pebrel_command' "$owner`n$line"
            [Console]::Write("$([char]27)]133;C$([char]7)")
        }
        $line
    }
}
