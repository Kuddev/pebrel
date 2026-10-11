[CmdletBinding()]
param([string] $InnoCompiler)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($PSVersionTable.PSEdition -eq 'Core') {
    # The inbox .NET Framework compiler embeds the reporter's DPI manifest.
    $arguments = @('-NoProfile', '-File', $PSCommandPath)
    if (-not [string]::IsNullOrWhiteSpace($InnoCompiler)) {
        $arguments += @('-InnoCompiler', $InnoCompiler)
    }
    & (Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\powershell.exe') @arguments
    if ($LASTEXITCODE -ne 0) { throw 'Native installer launch tests failed.' }
    return
}
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
if ([string]::IsNullOrWhiteSpace($InnoCompiler)) {
    $InnoCompiler = Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe'
}
if (-not (Test-Path -LiteralPath $InnoCompiler)) { throw "Inno compiler missing: $InnoCompiler" }
$root = Join-Path $repo ('tmp\installer-launch-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $root | Out-Null
$manifest = Join-Path $root 'dpi-child.manifest'
@'
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <application xmlns="urn:schemas-microsoft-com:asm.v3"><windowsSettings>
    <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
    <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
  </windowsSettings></application>
</assembly>
'@ | Set-Content -LiteralPath $manifest -Encoding utf8
$source = @'
using System;
using System.IO;
using System.Runtime.InteropServices;
class DpiChild {
    [DllImport("user32.dll")] static extern IntPtr GetDpiAwarenessContextForProcess(IntPtr process);
    [DllImport("user32.dll")] static extern bool AreDpiAwarenessContextsEqual(IntPtr a, IntPtr b);
    static int Main(string[] args) {
        IntPtr context = GetDpiAwarenessContextForProcess(new IntPtr(-1));
        string report = "v1=" + (AreDpiAwarenessContextsEqual(context, new IntPtr(-3)) ? "1" : "0") +
            "\nv2=" + (AreDpiAwarenessContextsEqual(context, new IntPtr(-4)) ? "1" : "0") +
            "\nargs=" + String.Join(" ", args);
        File.WriteAllText(Environment.GetEnvironmentVariable("PEBREL_LAUNCH_FIXTURE_REPORT"), report);
        return 0;
    }
}
'@
$provider = New-Object Microsoft.CSharp.CSharpCodeProvider
$parameters = New-Object System.CodeDom.Compiler.CompilerParameters
$parameters.GenerateExecutable = $true
$parameters.OutputAssembly = Join-Path $root 'dpi-child.exe'
$parameters.CompilerOptions = "/win32manifest:`"$manifest`""
$compiled = $provider.CompileAssemblyFromSource($parameters, $source)
if ($compiled.Errors.HasErrors) { throw ($compiled.Errors | Out-String) }
& $InnoCompiler '/Q' "/DFixtureRoot=$root" "/O$root" (Join-Path $PSScriptRoot 'installer-launch-fixture.iss')
if ($LASTEXITCODE -ne 0) { throw 'Launch fixture compilation failed.' }
$fixture = Join-Path $root 'installer-launch-fixture.exe'
foreach ($oldBehavior in @(1, 0)) {
    $process = Start-Process -FilePath $fixture -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', "/oldbehavior=$oldBehavior") -WindowStyle Hidden -Wait -PassThru
    $report = Get-Content -LiteralPath (Join-Path $root 'result.txt') -Raw -Encoding UTF8
    if ($oldBehavior -eq 1) {
        if ($report -ne 'FAIL: clean launch restores per-monitor V2') { throw "Regression control did not fail as expected: $report" }
    } elseif (-not $report.StartsWith('PASS: ')) {
        throw "Launch fixture failed: $report (exit $($process.ExitCode)); see $root"
    }
}
$scopeReport = $report
$process = Start-Process -FilePath $fixture -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/runentry=1') -WindowStyle Hidden -Wait -PassThru
$report = Get-Content -LiteralPath (Join-Path $root 'result.txt') -Raw -Encoding UTF8
if ($process.ExitCode -ne 0 -or $report -ne 'PASS: Run entry restored layer') {
    throw "Real Run entry failed: $report (exit $($process.ExitCode)); see $root"
}
$childPath = Join-Path $root 'child.txt'
$deadline = [datetime]::UtcNow.AddSeconds(5)
do {
    if (Test-Path -LiteralPath $childPath) {
        $childReport = Get-Content -LiteralPath $childPath -Raw -Encoding UTF8
        if ($childReport -match 'v2=1' -and $childReport -match 'args=--gpui') { break }
    }
    Start-Sleep -Milliseconds 100
} while ([datetime]::UtcNow -lt $deadline)
if (-not (Test-Path -LiteralPath $childPath) -or $childReport -notmatch 'v2=1') {
    throw "Real Run entry child did not restore V2; see $root"
}
$process = Start-Process -FilePath $fixture -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/deinit=1') -WindowStyle Hidden -Wait -PassThru
$report = Get-Content -LiteralPath (Join-Path $root 'result.txt') -Raw -Encoding UTF8
if ($report -ne 'PASS: shutdown restore') { throw "Shutdown restore failed: $report; see $root" }
Write-Output "installer-launch.tests.ps1: $scopeReport; real Run entry and shutdown restore PASS; old-behavior control failed as expected. Evidence: $root"
