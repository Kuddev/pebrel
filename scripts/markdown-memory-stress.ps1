<#
.SYNOPSIS
Exercise a dedicated Windows Markdown window and record process/GPU memory.
.DESCRIPTION
Generates original formulas, tables, code, lists and PNGs. HTTP images are served
only from generated assets on loopback. Samples private working set, total
working set and private commit separately; none is added to GPU counters.
No working-set trimming is used.
The default edit coordinates reproduce the fixture at 144 DPI and a 1938x1103
window. They scale with window DPI. Inspect first-cycle screenshots when changing
fonts, UI layout or the fixture; a click count alone does not prove editing.
Only this script is required. Reports, fixtures and screenshots belong in tmp/.
Run editing in a dedicated desktop session. Losing foreground focus stops the
edit run; screenshots capture only the owned window, including when occluded.
.EXAMPLE
./scripts/markdown-memory-stress.ps1 -Executable D:/build/pebrel.exe -BuildProfile release
.EXAMPLE
./scripts/markdown-memory-stress.ps1 -GenerateOnly -OutputDirectory D:/temp/markdown-fixture
#>
[CmdletBinding()]
param(
    [string]$Executable,
    [string]$OutputDirectory,
    [ValidateSet('release','dev','unknown')][string]$BuildProfile = 'unknown',
    [ValidateRange(1,1024)][int]$Sections = 128,
    [ValidateRange(1,10)][int]$Rounds = 3,
    [ValidateRange(1,200)][int]$Cycles = 40,
    [ValidateRange(1,10000)][int]$ScrollSteps = 550,
    [ValidateRange(80,2000)][int]$StepMilliseconds = 180,
    [ValidateRange(3,60)][int]$IdleSeconds = 20,
    [double]$WorkingSetLimitMiB = 100,
    [double]$TerminalLimitMiB = 70,
    [string[]]$EditTargets = @('formula:1130:358','table:645:508','code:750:632','list:720:834','inline-formula:756:885'),
    [switch]$GenerateOnly,
    [switch]$TerminalOnly,
    [switch]$SkipEditing,
    [ValidateSet('run','sample','serve')][string]$Mode = 'run',
    [int]$OwnerId = 0
)

$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$utf8 = [Text.UTF8Encoding]::new($false)
if (-not $OutputDirectory) {
    $OutputDirectory = Join-Path (Split-Path $PSScriptRoot) ('tmp/markdown-memory-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
}
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
function Write-Utf8([string]$Path, [string]$Text) { [IO.File]::WriteAllText($Path, $Text, $utf8) }
function Append-Json([string]$File, $Value) {
    [IO.File]::AppendAllText((Join-Path $OutputDirectory $File), ($Value | ConvertTo-Json -Compress -Depth 6) + "`n", $utf8)
}
function Median($Values) {
    $ordered = @($Values | Sort-Object)
    if ($ordered.Count -eq 0) { return $null }
    $middle = [int][Math]::Floor($ordered.Count/2)
    if ($ordered.Count%2) { return $ordered[$middle] }
    return ($ordered[$middle-1]+$ordered[$middle])/2
}

if ($Mode -eq 'serve') {
    $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
    $listener.Start()
    Write-Utf8 (Join-Path $OutputDirectory 'http-port.txt') ([string]$listener.LocalEndpoint.Port)
    try {
        while (-not (Test-Path (Join-Path $OutputDirectory 'stop-server'))) {
            if (-not $listener.Pending()) { Start-Sleep -Milliseconds 25; continue }
            $client = $listener.AcceptTcpClient()
            try {
                $stream = $client.GetStream()
                $stream.ReadTimeout = 2000
                $reader = [IO.StreamReader]::new($stream, [Text.Encoding]::ASCII, $false, 1024, $true)
                $request = $reader.ReadLine()
                while ($reader.ReadLine()) {}
                $reader.Dispose()
                $status = '404 Not Found'
                $body = [byte[]]@()
                if ($request -match '^GET /(image-[0-9]{2}\.png)(?:\?[^ ]*)? HTTP/1\.[01]$') {
                    $path = Join-Path (Join-Path $OutputDirectory 'assets') $Matches[1]
                    if (Test-Path -LiteralPath $path) { $body = [IO.File]::ReadAllBytes($path); $status = '200 OK' }
                }
                $headers = [Text.Encoding]::ASCII.GetBytes("HTTP/1.1 $status`r`nContent-Type: image/png`r`nContent-Length: $($body.Length)`r`nConnection: close`r`n`r`n")
                $stream.Write($headers, 0, $headers.Length)
                $stream.Write($body, 0, $body.Length)
                Append-Json 'http-requests.jsonl' @{time=(Get-Date).ToString('o');request=$request;status=$status}
            } catch { Append-Json 'http-errors.jsonl' @{error=$_.Exception.Message} }
            finally { $client.Dispose() }
        }
    } finally { $listener.Stop() }
    exit
}

if ($Mode -eq 'sample') {
    $timer = [Diagnostics.Stopwatch]::StartNew()
    $gpuAvailable = $true
    $ready = $false
    while (-not (Test-Path (Join-Path $OutputDirectory "stop-sampling-$OwnerId"))) {
        $target = Get-Process -Id $OwnerId -ErrorAction SilentlyContinue
        if ($null -eq $target) { break }
        $target.Refresh()
        $phaseFile = Join-Path $OutputDirectory 'phase.txt'
        $phase = if (Test-Path $phaseFile) { (Get-Content $phaseFile -Raw).Trim() } else { 'startup' }
        $dedicated = $null; $shared = $null
        $gpuTimer = [Diagnostics.Stopwatch]::StartNew()
        if ($gpuAvailable) {
            try {
                $gpu = @(Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUProcessMemory -Filter "Name LIKE 'pid_${OwnerId}_%'" -ErrorAction Stop)
                if ($gpu.Count) {
                    $dedicated = [long](($gpu | Measure-Object DedicatedUsage -Sum).Sum)
                    $shared = [long](($gpu | Measure-Object SharedUsage -Sum).Sum)
                }
            } catch { $gpuAvailable = $false; Append-Json 'limitations.jsonl' @{gpu=$_.Exception.Message} }
        }
        $gpuTimer.Stop()
        $privateWorkingSet = $null
        try {
            $resident = @(Get-CimInstance Win32_PerfFormattedData_PerfProc_Process -Filter "IDProcess=$OwnerId" -ErrorAction Stop)
            if ($resident.Count -ne 1 -or $null -eq $resident[0].WorkingSetPrivate) {
                throw 'The private working-set counter is missing or ambiguous.'
            }
            $privateWorkingSet = [long]$resident[0].WorkingSetPrivate
        } catch { Append-Json 'limitations.jsonl' @{privateWorkingSet=$_.Exception.Message} }
        # Counter discovery can be slow. Read the process and phase again
        # afterwards so a delayed query cannot label a new phase as an old one.
        $target.Refresh()
        $phase = if (Test-Path $phaseFile) { (Get-Content $phaseFile -Raw).Trim() } else { 'startup' }
        $panic = Test-Path (Join-Path $OutputDirectory 'pebrel-panic.log')
        [pscustomobject]@{
            time=(Get-Date).ToString('o');pid=$OwnerId;seconds=$timer.Elapsed.TotalSeconds;phase=$phase
            privateBytes=$target.PrivateMemorySize64;workingSetBytes=$target.WorkingSet64
            privateWorkingSetBytes=$privateWorkingSet
            peakWorkingSetBytes=$target.PeakWorkingSet64;cpuSeconds=$target.TotalProcessorTime.TotalSeconds
            handles=$target.HandleCount;threads=$target.Threads.Count
            gpuDedicatedBytes=$dedicated;gpuSharedBytes=$shared;valid=(-not $panic)
            gpuQueryMilliseconds=$gpuTimer.Elapsed.TotalMilliseconds
        } | Export-Csv -NoTypeInformation -Encoding UTF8 -Append (Join-Path $OutputDirectory 'samples.csv')
        if (-not $ready) {
            Write-Utf8 (Join-Path $OutputDirectory 'sampler-ready') 'ready'
            $ready = $true
        }
        if ($panic) { break }
        Start-Sleep -Seconds 2
    }
    exit
}

if (Test-Path $OutputDirectory) { throw 'Use a new output directory; existing evidence is never overwritten.' }
$null = New-Item -ItemType Directory -Path (Join-Path $OutputDirectory 'assets') -Force
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class MarkdownStressWindow {
 [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left,Top,Right,Bottom; }
 [StructLayout(LayoutKind.Sequential)] public struct Point { public int X,Y; }
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out Rect r);
 [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h,ref Point p);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern short GetAsyncKeyState(int key);
 [DllImport("user32.dll")] public static extern uint MapVirtualKey(uint code,uint type);
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h,IntPtr dc,uint flags);
 [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h,int x,int y,int w,int z,bool repaint);
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h,int m,IntPtr w,IntPtr l);
}
'@
[void][MarkdownStressWindow]::SetProcessDPIAware()
$hostExecutable = (Get-Process -Id $PID).Path
function Start-Worker([string]$WorkerMode, [int]$Id = 0) {
    Start-Process $hostExecutable -PassThru -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-File',('"' + $PSCommandPath + '"'),'-Mode',$WorkerMode,'-OutputDirectory',('"' + $OutputDirectory + '"'),'-OwnerId',$Id) -WindowStyle Hidden
}
function Phase([string]$Name) {
    Write-Utf8 (Join-Path $OutputDirectory 'phase.txt') $Name
    Append-Json 'actions.jsonl' @{time=(Get-Date).ToString('o');phase=$Name}
    Write-Output $Name
}
function Assert-Running {
    $script:process.Refresh()
    if ($script:process.HasExited -or (Test-Path (Join-Path $OutputDirectory 'pebrel-panic.log'))) {
        throw 'The owned test process exited or panicked; automation has stopped.'
    }
}
function Pause-Checked([int]$Milliseconds) {
    $remaining = $Milliseconds
    while ($remaining -gt 0) {
        Assert-Running
        $step = [Math]::Min(200, $remaining)
        Start-Sleep -Milliseconds $step
        $remaining -= $step
    }
}
function Key([int]$Code) {
    Assert-Running
    $scan = [long][MarkdownStressWindow]::MapVirtualKey($Code,0)
    $down = 1 -bor ($scan -shl 16)
    [void][MarkdownStressWindow]::PostMessage($script:handle,0x100,[IntPtr]$Code,[IntPtr]$down)
    [void][MarkdownStressWindow]::PostMessage($script:handle,0x101,[IntPtr]$Code,[IntPtr]($down -bor 0xC0000000L))
}
function Assert-EditingFocus {
    if ([MarkdownStressWindow]::GetForegroundWindow() -ne $script:handle) {
        throw 'Editing lost foreground focus. Use a dedicated desktop session, or -SkipEditing for scrolling only.'
    }
    foreach ($key in @(16,17,18,91,92)) {
        if ([MarkdownStressWindow]::GetAsyncKeyState($key) -lt 0) { throw 'Physical modifier input interrupted automation.' }
    }
}
function Click([int]$X,[int]$Y,[switch]$Middle) {
    Assert-Running
    $position = [IntPtr](([int]($Y*$script:scale) -shl 16) -bor [int]($X*$script:scale))
    [void][MarkdownStressWindow]::PostMessage($script:handle,0x200,[IntPtr]0,$position)
    $down = if ($Middle) { 0x207 } else { 0x201 }
    $up = if ($Middle) { 0x208 } else { 0x202 }
    $button = if ($Middle) { 16 } else { 1 }
    [void][MarkdownStressWindow]::PostMessage($script:handle,$down,[IntPtr]$button,$position)
    [void][MarkdownStressWindow]::PostMessage($script:handle,$up,[IntPtr]0,$position)
}
function Capture([string]$Name) {
    Assert-Running
    Pause-Checked 150
    $rect = New-Object MarkdownStressWindow+Rect
    [void][MarkdownStressWindow]::GetWindowRect($script:handle,[ref]$rect)
    $bitmap = New-Object Drawing.Bitmap ($rect.Right-$rect.Left),($rect.Bottom-$rect.Top)
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    try {
        $dc = $graphics.GetHdc()
        try { $captured = [MarkdownStressWindow]::PrintWindow($script:handle,$dc,2) }
        finally { $graphics.ReleaseHdc($dc) }
        if (-not $captured) { throw 'The owned window could not be captured.' }
        $bitmap.Save((Join-Path $OutputDirectory ($Name + '.png')),[Drawing.Imaging.ImageFormat]::Png)
    } finally { $graphics.Dispose(); $bitmap.Dispose() }
}
function Check-Accessibility {
    Assert-Running
    $element = [Windows.Automation.AutomationElement]::FromHandle($script:handle)
    $nodes = $element.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)
    Append-Json 'accessibility.jsonl' @{time=(Get-Date).ToString('o');nodes=$nodes.Count;pid=$script:process.Id}
    Pause-Checked 500
}
function Assert-Surface([bool]$DocumentExpected) {
    Assert-Running
    $element = [Windows.Automation.AutomationElement]::FromHandle($script:handle)
    $nodes = $element.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)
    $save = @($nodes | Where-Object { $_.Current.Name -eq '保存' -and $_.Current.ControlType -eq [Windows.Automation.ControlType]::Button })
    $dialogs = @($nodes | Where-Object { $_.Current.ControlType -eq [Windows.Automation.ControlType]::Window })
    if ($dialogs.Count -or (($save.Count -gt 0) -ne $DocumentExpected)) {
        throw 'The requested document/terminal surface was not reached, or a modal dialog is open.'
    }
    Append-Json 'surface-checks.jsonl' @{time=(Get-Date).ToString('o');document=$DocumentExpected;nodes=$nodes.Count}
}
function Scroll([int]$Delta) {
    Key 27
    $point = New-Object MarkdownStressWindow+Point
    $point.X = [int](1000*$script:scale); $point.Y = [int](700*$script:scale)
    [void][MarkdownStressWindow]::PostMessage($script:handle,0x200,[IntPtr]0,[IntPtr](($point.Y -shl 16) -bor $point.X))
    [void][MarkdownStressWindow]::ClientToScreen($script:handle,[ref]$point)
    $position = [IntPtr](($point.Y -shl 16) -bor ($point.X -band 65535))
    for ($step = 0; $step -lt $ScrollSteps; $step++) {
        Assert-Running
        [void][MarkdownStressWindow]::PostMessage($script:handle,0x20A,[IntPtr]($Delta -shl 16),$position)
        Pause-Checked $StepMilliseconds
    }
}

$server = $null; $sampler = $null; $script:process = $null
$oldConfig = $env:PEBREL_CONFIG_DIR; $oldDocument = $env:NEBULA_GPUI_OPEN_DOC
$success = $false
try {
    for ($i = 0; $i -lt 32; $i++) {
        $bitmap = New-Object Drawing.Bitmap 1024,640
        $graphics = [Drawing.Graphics]::FromImage($bitmap)
        for ($x = 0; $x -lt 1024; $x += 8) {
            $brush = New-Object Drawing.SolidBrush ([Drawing.Color]::FromArgb(([int][Math]::Floor($x/8)+$i*13)%256,([int][Math]::Floor($x/4)+$i*19)%256,([int][Math]::Floor($x/16)+$i*29)%256))
            $graphics.FillRectangle($brush,$x,0,8,640)
            $brush.Dispose()
        }
        $bitmap.Save((Join-Path $OutputDirectory ('assets/image-{0:d2}.png' -f $i)),[Drawing.Imaging.ImageFormat]::Png)
        $graphics.Dispose(); $bitmap.Dispose()
    }
    $server = Start-Worker 'serve'
    for ($retry = 0; $retry -lt 100 -and -not (Test-Path (Join-Path $OutputDirectory 'http-port.txt')); $retry++) { Start-Sleep -Milliseconds 100 }
    $port = [int](Get-Content (Join-Path $OutputDirectory 'http-port.txt'))
    $intro = @'
# 混合文档内存检查

反复点击公式、表格、代码和列表，观察编辑控件的创建与回收。

$$
E = mc^2
$$

| 内容 | 操作 |
| :--- | :--- |
| 单元格 | 直接输入 |

```rust
fn check() {
    let value = 42;
}
```

- [ ] 任务项目
- 普通列表项目

行内公式 $a^2 + b^2 = c^2$，以及 [图片链接](http://127.0.0.1:PORT/image-00.png)。

'@
    $template = @'

## 内容段落 NUMBER

这是第 NUMBER 组独立内容，包含 **强调**、*说明*、`行内代码` 和 [图片链接](assets/IMAGE)。
行内表达式 $x_{NUMBER} + y = NEXT$。

$$
\frac{1}{NUMBER} \sum_{k=1}^{NEXT} k^2
$$

$$
\int_0^1 x^{POWER} \, dx = \frac{1}{DENOMINATOR} \quad n=NUMBER
$$

| 项目 | 内容 | 次数 |
| :--- | :--- | ---: |
| 编辑 | 单元格保持边框 | NUMBER |
| 滚动 | 只挂载可见内容 | NEXT |

```rust
fn item_INDEX() -> usize {
    let values = [1, 2, 3, NUMBER];
    values.iter().sum()
}
```

- [ ] 检查第 NUMBER 个项目
- 保留原有列表结构

![生成的测试图片 NUMBER](URI)

'@
    $source = [Text.StringBuilder]::new($intro.Replace('PORT',[string]$port))
    for ($i = 0; $i -lt $Sections; $i++) {
        $name = 'image-{0:d2}.png' -f ($i%32)
        $uri = if ($i%2) { "http://127.0.0.1:$port/${name}?image=$i" } else { "assets/$name" }
        [void]$source.Append($template.Replace('NUMBER',[string]($i+1)).Replace('NEXT',[string]($i+2)).Replace('POWER',[string]($i%9+1)).Replace('DENOMINATOR',[string]($i%9+2)).Replace('INDEX',[string]$i).Replace('IMAGE',$name).Replace('URI',$uri))
    }
    $document = Join-Path $OutputDirectory 'mixed.md'
    Write-Utf8 $document $source.ToString()
    $manifest = [ordered]@{
        runnerSha256=(Get-FileHash $PSCommandPath -Algorithm SHA256).Hash
        sections=$Sections;displayFormulas=(2*$Sections+1);inlineFormulas=($Sections+1)
        tables=($Sections+1);codeBlocks=($Sections+1);images=$Sections;imagePixels=@(1024,640)
        rounds=$Rounds;editCycles=$Cycles;editTargets=$EditTargets;skipEditing=[bool]$SkipEditing
        scrollSteps=$ScrollSteps;stepMilliseconds=$StepMilliseconds;profile=$BuildProfile
        fixtureSha256=(Get-FileHash $document -Algorithm SHA256).Hash
        workingSetLimitMiB=$WorkingSetLimitMiB;terminalLimitMiB=$TerminalLimitMiB;terminalOnly=[bool]$TerminalOnly
        measurement='Separate private working set, total working set, private commit and GPU counters; HTTP loopback, no WAN latency claim.'
    }
    if ($GenerateOnly) { $success = $true; Write-Utf8 (Join-Path $OutputDirectory 'manifest.json') ($manifest | ConvertTo-Json -Depth 6); return }
    $Executable = (Resolve-Path -LiteralPath $Executable).Path
    $manifest.executable = $Executable
    $manifest.executableSha256 = (Get-FileHash $Executable -Algorithm SHA256).Hash
    $manifest.os = (Get-CimInstance Win32_OperatingSystem | Select-Object Caption,Version,TotalVisibleMemorySize)
    Write-Utf8 (Join-Path $OutputDirectory 'manifest.json') ($manifest | ConvertTo-Json -Depth 6)
    Write-Utf8 (Join-Path $OutputDirectory 'pebrel_settings.txt') (@('language=zh-CN','theme=SilverLight','opacity=1','blur=off','restore_session=false','resume_ai=false','windowing_behavior=UseNew','panel_resize=on','auto_check_updates=off',"startup_directory=$OutputDirectory") -join "`n")
    $env:PEBREL_CONFIG_DIR = $OutputDirectory
    $env:NEBULA_GPUI_OPEN_DOC = if ($TerminalOnly) { $null } else { $document }
    $script:process = Start-Process $Executable -WorkingDirectory $OutputDirectory -PassThru -ArgumentList '-e','cmd.exe','/d','/k','prompt MemoryCheck' -RedirectStandardOutput (Join-Path $OutputDirectory 'stdout.log') -RedirectStandardError (Join-Path $OutputDirectory 'stderr.log')
    Write-Utf8 (Join-Path $OutputDirectory 'pid.txt') ([string]$script:process.Id)
    for ($retry = 0; $retry -lt 100; $retry++) {
        Assert-Running
        if ($script:process.MainWindowHandle -ne 0) { break }
        Start-Sleep -Milliseconds 100
    }
    $script:handle = $script:process.MainWindowHandle
    if ($script:handle -eq 0) { throw 'The test window did not appear.' }
    $script:scale = [MarkdownStressWindow]::GetDpiForWindow($script:handle)/144.0
    [void][MarkdownStressWindow]::MoveWindow($script:handle,0,0,[int](1938*$script:scale),[int](1103*$script:scale),$true)
    $sampler = Start-Worker 'sample' $script:process.Id
    # Let one counter query finish before starting measured stages. Initial GPU
    # discovery can otherwise consume the entire first idle interval.
    for ($retry = 0; $retry -lt 600 -and -not (Test-Path (Join-Path $OutputDirectory 'sampler-ready')); $retry++) {
        if ($sampler.HasExited) { throw 'The memory sampler exited before its first sample.' }
        Pause-Checked 100
    }
    if (-not (Test-Path (Join-Path $OutputDirectory 'sampler-ready'))) { throw 'The memory sampler did not become ready within 60 seconds.' }
    if (-not $SkipEditing -and -not $TerminalOnly) {
        if ([MarkdownStressWindow]::GetForegroundWindow() -ne $script:handle -and -not [MarkdownStressWindow]::SetForegroundWindow($script:handle)) {
            throw 'Windows did not grant foreground focus to the edit-test window.'
        }
        Pause-Checked 100
        Assert-EditingFocus
    }
    if ($TerminalOnly) {
        Phase 'terminal-never-opened-markdown'
        Pause-Checked ($IdleSeconds*1000)
        Capture 'terminal-baseline'
        $success = $true
        return
    }
    Phase 'markdown-initial'
    Pause-Checked ($IdleSeconds*1000)
    Check-Accessibility
    Assert-Surface $true
    Capture 'initial'
    for ($round = 1; $round -le $Rounds; $round++) {
        if (-not $SkipEditing) {
            Phase "round-$round-edit"
            for ($cycle = 0; $cycle -lt $Cycles; $cycle++) {
                foreach ($target in $EditTargets) {
                    Assert-EditingFocus
                    $parts = $target.Split(':')
                    Click ([int]$parts[1]) ([int]$parts[2])
                    Pause-Checked 100
                    [void][MarkdownStressWindow]::PostMessage($script:handle,0x102,[IntPtr]81,[IntPtr]1)
                    Pause-Checked 100
                    if ($cycle -eq 0) { Capture "round-$round-$($parts[0])-typed" }
                    Assert-EditingFocus
                    Key 8
                    Pause-Checked 100
                    Key 27
                    Pause-Checked 80
                }
            }
        }
        Phase "round-$round-down"
        Scroll -600
        Phase "round-$round-bottom"
        Pause-Checked ($IdleSeconds*1000)
        Capture "round-$round-bottom"
        Check-Accessibility
        Phase "round-$round-up"
        Scroll 600
        Phase "round-$round-top"
        Pause-Checked ($IdleSeconds*1000)
        Capture "round-$round-top"
    }
    Phase 'markdown-tab-inactive'
    Click 140 170
    Pause-Checked 500
    Assert-Surface $false
    Pause-Checked ($IdleSeconds*1000)
    Capture 'cli-with-markdown-background'
    Phase 'markdown-resumed'
    Click 150 220
    Pause-Checked 500
    Assert-Surface $true
    Pause-Checked ($IdleSeconds*1000)
    Check-Accessibility
    Capture 'resumed'
    Phase 'verify-edit-restoration'
    Click 1850 110
    Pause-Checked 1000
    if ((Get-FileHash $document -Algorithm SHA256).Hash -ne $manifest.fixtureSha256) {
        throw 'Edit/delete did not restore the generated document. The saved draft is retained for diagnosis.'
    }
    Click 150 220 -Middle
    Pause-Checked 500
    Assert-Surface $false
    Phase 'markdown-closed'
    Pause-Checked ($IdleSeconds*1000)
    Capture 'closed'
    $success = $true
} catch {
    $phasePath = Join-Path $OutputDirectory 'phase.txt'
    $failedPhase = if ([IO.File]::Exists($phasePath)) { [IO.File]::ReadAllText($phasePath,$utf8).Trim() } else { 'startup' }
    Append-Json 'failure.jsonl' @{time=(Get-Date).ToString('o');message=$_.Exception.Message;phase=$failedPhase}
    Write-Warning $_.Exception.Message
} finally {
    Write-Utf8 (Join-Path $OutputDirectory 'stop-server') 'stop'
    if ($script:process) { Write-Utf8 (Join-Path $OutputDirectory "stop-sampling-$($script:process.Id)") 'stop' }
    $samplingComplete = if ($sampler) { $sampler.WaitForExit(5000) -and $sampler.ExitCode -eq 0 } else { $false }
    if ($server) { [void]$server.WaitForExit(3000) }
    $env:PEBREL_CONFIG_DIR = $oldConfig; $env:NEBULA_GPUI_OPEN_DOC = $oldDocument
    $rows = @()
    $sampleFile = Join-Path $OutputDirectory 'samples.csv'
    if (Test-Path $sampleFile) { $rows = @(Import-Csv $sampleFile | Where-Object valid -eq 'True') }
    $phases = @($rows | Group-Object phase | ForEach-Object {
        $working = @($_.Group | ForEach-Object { [double]$_.workingSetBytes/1MB } | Sort-Object)
        $private = @($_.Group | ForEach-Object { [double]$_.privateBytes/1MB } | Sort-Object)
        $privateWorking = @($_.Group | Where-Object privateWorkingSetBytes -ne '' | ForEach-Object { [double]$_.privateWorkingSetBytes/1MB } | Sort-Object)
        $privateWorkingComplete = $privateWorking.Count -eq $_.Group.Count
        $dedicated = @($_.Group | Where-Object gpuDedicatedBytes -ne '' | ForEach-Object { [double]$_.gpuDedicatedBytes/1MB })
        $shared = @($_.Group | Where-Object gpuSharedBytes -ne '' | ForEach-Object { [double]$_.gpuSharedBytes/1MB })
        $limit = if ($TerminalOnly -or $_.Name -in @('markdown-tab-inactive','markdown-closed')) { $TerminalLimitMiB } else { $WorkingSetLimitMiB }
        [pscustomobject]@{
            phase=$_.Name;samples=$working.Count
            workingMedianMiB=(Median $working);workingMaxMiB=$working[-1]
            privateMedianMiB=(Median $private);privateMaxMiB=$private[-1]
            privateWorkingMedianMiB=(Median $privateWorking)
            privateWorkingMaxMiB=if ($privateWorking.Count) { $privateWorking[-1] } else { $null }
            privateWorkingSetComplete=$privateWorkingComplete
            privateWorkingSetWithinTarget=($privateWorkingComplete -and $privateWorking[-1] -le $limit)
            gpuDedicatedMedianMiB=(Median $dedicated);gpuSharedMedianMiB=(Median $shared)
            processLimitMiB=$limit;bothProcessMetricsWithinTarget=($working[-1] -le $limit -and $private[-1] -le $limit)
        }
    })
    $budget = if ($TerminalOnly) { $TerminalLimitMiB } else { $WorkingSetLimitMiB }
    $overBudget = @($rows | Where-Object { [double]$_.workingSetBytes/1MB -gt $budget })
    $processTargetsPassed = $phases.Count -gt 0 -and @($phases | Where-Object bothProcessMetricsWithinTarget -eq $false).Count -eq 0
    $privateWorkingComplete = $phases.Count -gt 0 -and @($phases | Where-Object privateWorkingSetComplete -eq $false).Count -eq 0
    $privateWorkingTargetsPassed = $phases.Count -gt 0 -and @($phases | Where-Object privateWorkingSetWithinTarget -eq $false).Count -eq 0
    $requiredPhases = if ($TerminalOnly) { @('terminal-never-opened-markdown') } else {
        @('markdown-initial','markdown-tab-inactive','markdown-resumed','markdown-closed') + @(for ($round = 1; $round -le $Rounds; $round++) { "round-$round-bottom"; "round-$round-top" })
    }
    $missingPhases = @($requiredPhases | Where-Object { $_ -notin $phases.phase })
    $phaseCoverageComplete = $missingPhases.Count -eq 0
    Write-Utf8 (Join-Path $OutputDirectory 'summary.json') (@{
        completed=$success;samplingComplete=$samplingComplete;phases=$phases
        phaseCoverageComplete=$phaseCoverageComplete;missingPhases=$missingPhases
        workingSetLimitMiB=$budget;workingSetBudgetPassed=($success -and $samplingComplete -and $phaseCoverageComplete -and $rows.Count -gt 0 -and $overBudget.Count -eq 0)
        privateWorkingSetSamplingComplete=$privateWorkingComplete
        privateWorkingSetBudgetPassed=($success -and $samplingComplete -and $phaseCoverageComplete -and $privateWorkingTargetsPassed)
        processMemoryBudgetPassed=($success -and $samplingComplete -and $phaseCoverageComplete -and $processTargetsPassed -and $privateWorkingTargetsPassed)
        requiresVisualCoverageReview=$true;limitsAreTargetsNotGuarantees=$true
    } | ConvertTo-Json -Depth 6)
    Write-Output "Evidence: $OutputDirectory"
}
if (-not $success) { exit 1 }
