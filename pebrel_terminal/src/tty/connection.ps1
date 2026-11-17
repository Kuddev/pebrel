# Use the application's actual arguments after PowerShell has expanded variables
# and arrays. Console writes stay outside PowerShell's success-output pipeline.
function global:Invoke-PebrelConnection {
    param([string]$Program, [object[]]$Arguments)
    $application = Get-Command -Name $Program -CommandType Application -ErrorAction Stop | Select-Object -First 1
    $token = $global:PebrelShellToken
    $owner = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($token))
    $words = @($Program) + @($Arguments | ForEach-Object { [string]$_ })
    $payload = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($owner + "`n" + ($words -join [char]0)))
    [Console]::Write("$([char]27)]1337;SetUserVar=pebrel_shell=$token$([char]7)")
    [Console]::Write("$([char]27)]1337;SetUserVar=pebrel_connection=$payload$([char]7)")
    try {
        & $application @Arguments
    } finally {
        [Console]::Write("$([char]27)]1337;SetUserVar=pebrel_shell=$token$([char]7)")
    }
}

# A user's function/alias keeps precedence. Fully qualified executable paths
# also remain available as an explicit way to invoke the application directly.
if ((Get-Command ssh -ErrorAction SilentlyContinue).CommandType -eq 'Application') {
    function global:ssh { Invoke-PebrelConnection 'ssh' $args }
}
if ((Get-Command ssh.exe -ErrorAction SilentlyContinue).CommandType -eq 'Application') {
    function global:ssh.exe { Invoke-PebrelConnection 'ssh' $args }
}
if ((Get-Command wsl -ErrorAction SilentlyContinue).CommandType -eq 'Application') {
    function global:wsl { Invoke-PebrelConnection 'wsl' $args }
}
if ((Get-Command wsl.exe -ErrorAction SilentlyContinue).CommandType -eq 'Application') {
    function global:wsl.exe { Invoke-PebrelConnection 'wsl' $args }
}
