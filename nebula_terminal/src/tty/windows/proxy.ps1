# Rust selects and validates the proxy. SOCKS URLs stay in the child environment;
# only the explicit HTTP marker configures PowerShell's HTTP request cmdlets.
& {
    if (-not $env:PEBREL_HTTP_PROXY) { return }
    try {
        $address = [Uri]$env:PEBREL_HTTP_PROXY
        if ($address.Scheme -ne 'http') { throw 'Expected an HTTP proxy' }
        $builder = [UriBuilder]$address
        $builder.UserName = ''
        $builder.Password = ''
        $proxyUrl = $builder.Uri.AbsoluteUri
        $credential = $null
        if ($address.UserInfo) {
            $parts = $address.UserInfo.Split([char[]]@(':'), 2)
            $user = [Uri]::UnescapeDataString($parts[0])
            $password = if ($parts.Length -gt 1) { [Uri]::UnescapeDataString($parts[1]) } else { '' }
            $secret = New-Object System.Security.SecureString
            foreach ($character in $password.ToCharArray()) { $secret.AppendChar($character) }
            $secret.MakeReadOnly()
            $credential = New-Object System.Management.Automation.PSCredential($user, $secret)
        }
        foreach ($command in @('Invoke-WebRequest', 'Invoke-RestMethod')) {
            $global:PSDefaultParameterValues["${command}:Proxy"] = $proxyUrl
            $global:PSDefaultParameterValues.Remove("${command}:ProxyCredential")
            if ($credential) {
                $global:PSDefaultParameterValues["${command}:ProxyCredential"] = $credential
            }
        }
        $pebrelIsPwsh = ($PSVersionTable.PSEdition -eq 'Core') -or ($PSVersionTable.PSVersion.Major -ge 6)
        if (-not $pebrelIsPwsh) {
            $proxy = New-Object System.Net.WebProxy($proxyUrl, $true)
            if ($credential) { $proxy.Credentials = $credential.GetNetworkCredential() }
            [System.Net.WebRequest]::DefaultWebProxy = $proxy
        }
    } catch {
        # Do not print a URL or exception that could contain proxy credentials.
        Write-Warning 'Could not configure the terminal HTTP proxy.'
    }
}
