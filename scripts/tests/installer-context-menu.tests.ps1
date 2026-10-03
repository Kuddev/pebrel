[CmdletBinding()]
param([Parameter(Mandatory)][string] $InnoCompiler)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$root = Join-Path $repo ('tmp\explorer-menu-' + [guid]::NewGuid().ToString('N'))
$registryRoot = 'Software\PebrelTestFixtures\' + (Split-Path $root -Leaf)
New-Item -ItemType Directory -Path $root | Out-Null
& $InnoCompiler '/Q' "/DFixtureRoot=$root" "/O$root" (Join-Path $PSScriptRoot 'installer-context-menu-fixture.iss')
if ($LASTEXITCODE -ne 0) { throw 'Explorer fixture compilation failed.' }
$executable = Join-Path $root 'installer-context-menu-fixture.exe'
$process = Start-Process $executable -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES' -Wait -PassThru
$report = Get-Content (Join-Path $root 'choices-result.txt') -Raw -Encoding UTF8
if ($process.ExitCode -ne 0 -or -not $report.StartsWith('PASS: ')) { throw $report }
Write-Output $report

Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class ExplorerFixtureInput {
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct Point { public int X, Y; }
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr hwnd, ref Point point);
    [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr hwnd, uint message, IntPtr row, ref Rect rect);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out Rect rect);
    [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr hwnd, uint flags);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint processId);
    [DllImport("user32.dll")] public static extern bool RedrawWindow(IntPtr hwnd, IntPtr rect, IntPtr region, uint flags);
    [DllImport("dwmapi.dll")] public static extern int DwmFlush();
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint x, uint y, uint data, UIntPtr extra);
}
'@

function Wait-ForFile([string] $path, $process) {
    $deadline = [datetime]::UtcNow.AddSeconds(20)
    while (-not (Test-Path $path)) {
        if ($process.HasExited -or [datetime]::UtcNow -gt $deadline) { throw "Fixture did not reach $path" }
        Start-Sleep -Milliseconds 100
    }
}

function Click-Row([IntPtr] $handle, [int] $row) {
    $rect = [ExplorerFixtureInput+Rect]::new()
    if ([ExplorerFixtureInput]::SendMessage($handle, 0x198, [IntPtr]$row, [ref]$rect).ToInt64() -lt 0) {
        throw "Cannot locate checkbox row $row"
    }
    $point = [ExplorerFixtureInput+Point]::new()
    $point.X = 8
    $point.Y = ($rect.Top + $rect.Bottom) / 2
    [void][ExplorerFixtureInput]::ClientToScreen($handle, [ref]$point)
    [void][ExplorerFixtureInput]::SetCursorPos($point.X, $point.Y)
    [ExplorerFixtureInput]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero)
    [ExplorerFixtureInput]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 100
}

function Save-Window([IntPtr] $windowHandle, [string] $name) {
    [void][ExplorerFixtureInput]::RedrawWindow($windowHandle, [IntPtr]::Zero, [IntPtr]::Zero, 0x181)
    [void][ExplorerFixtureInput]::DwmFlush()
    $rect = [ExplorerFixtureInput+Rect]::new()
    [void][ExplorerFixtureInput]::GetWindowRect($windowHandle, [ref]$rect)
    $bitmap = [Drawing.Bitmap]::new($rect.Right - $rect.Left, $rect.Bottom - $rect.Top)
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bitmap.Size)
        $bitmap.Save((Join-Path $root "$name.png"))
    } finally {
        $graphics.Dispose()
        $bitmap.Dispose()
    }
}

function Run-Ui([string] $phase, [scriptblock] $actions) {
    Remove-Item (Join-Path $root 'ui-handle.txt'), (Join-Path $root 'ui-result.txt') -ErrorAction SilentlyContinue
    $process = Start-Process $executable -ArgumentList '/ExplorerUi=1' -PassThru
    try {
        Wait-ForFile (Join-Path $root 'ui-handle.txt') $process
        $handle = [IntPtr][long](Get-Content (Join-Path $root 'ui-handle.txt') -Raw)
        $windowHandle = [ExplorerFixtureInput]::GetAncestor($handle, 2)
        $windowProcessId = [uint32]0
        [void][ExplorerFixtureInput]::GetWindowThreadProcessId($windowHandle, [ref]$windowProcessId)
        if (-not [Diagnostics.Process]::GetProcessById($windowProcessId).WaitForInputIdle(20000)) {
            throw 'Installer did not complete its initial UI rendering.'
        }
        [void][ExplorerFixtureInput]::SetForegroundWindow($windowHandle)
        Save-Window $windowHandle "$phase-before"
        & $actions $handle
        Save-Window $windowHandle "$phase-after"
        [Windows.Forms.SendKeys]::SendWait('%n')
        Wait-ForFile (Join-Path $root 'ui-result.txt') $process
        if (-not $process.WaitForExit(20000) -or $process.ExitCode -ne 0) { throw 'UI fixture failed to finish.' }
    } finally {
        if (-not $process.HasExited) { $process.Kill() }
    }
}

try {
    Run-Ui 'choose' { param($handle) Click-Row $handle 3 }
    $choices = Get-ItemProperty "HKCU:\$registryRoot\Choices"
    if ($choices.Enabled -ne 1 -or $choices.Default -ne 1 -or
        $choices.'wsl:Ubuntu Test' -ne 1 -or $choices.'wsl:开发环境' -ne 0) { throw 'Mouse checkbox selection was not saved.' }
    foreach ($scope in @('selected', 'background')) {
        $children = @(Get-ChildItem "HKCU:\$registryRoot\$scope\PebrelWslMenu\shell")
        if ($children.Count -ne 1) { throw 'UI did not remove the deselected distribution.' }
    }
    Run-Ui 'disable' { param($handle)
        # Keyboard operates the actual focused checklist, including the master switch.
        [Windows.Forms.SendKeys]::SendWait('{HOME} ')
        Click-Row $handle 2
    }
    $choices = Get-ItemProperty "HKCU:\$registryRoot\Choices"
    if ($choices.Enabled -ne 0 -or $choices.'wsl:Ubuntu Test' -ne 1) { throw 'Master toggle lost or changed a disabled choice.' }
    foreach ($scope in @('selected', 'background')) {
        if (Test-Path "HKCU:\$registryRoot\$scope\Pebrel") { throw 'Disabled ordinary entry remained.' }
        if (Test-Path "HKCU:\$registryRoot\$scope\PebrelWslMenu") { throw 'Disabled WSL entry remained.' }
    }
    Run-Ui 'restore' { param($handle) Click-Row $handle 0 }
    $choices = Get-ItemProperty "HKCU:\$registryRoot\Choices"
    if ($choices.Enabled -ne 1 -or $choices.'wsl:开发环境' -ne 0) { throw 'Upgrade changed the stored selection.' }
    foreach ($scope in @('selected', 'background')) {
        if (@(Get-ChildItem "HKCU:\$registryRoot\$scope\PebrelWslMenu\shell").Count -ne 1) { throw 'Re-enable did not restore only the selected distribution.' }
    }
    Write-Output 'PASS: native installer mouse selection, keyboard master toggle, disabled input, persistence and restore'
    Write-Output "Fixture evidence: $root"
} finally {
    Remove-Item "HKCU:\$registryRoot" -Recurse -Force -ErrorAction SilentlyContinue
}
