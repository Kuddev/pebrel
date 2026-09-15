# C1-C12 验收驱动：向独立探针实例 PostMessage 键盘事件（不抢用户焦点），
# 用 PrintWindow 截图存证。用法：
#   powershell -File scripts/capture_acceptance.ps1 -Exe <exe> -OutPrefix <tag>
#   -Keys @('ctrl-shift-p','a','enter')  每个元素 = 修饰键+键名或单键
#   -SettleMs 每次按键后的等待
param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [Parameter(Mandatory = $true)][string]$OutPrefix,
    [string[]]$Keys = @(),
    [int]$SettleMs = 900,
    [string]$WorkingDir = '',
    [string]$ExtraArgs = '',
    [int]$WarmupSec = 8,
    [int]$WindowW = 0,
    [int]$WindowH = 0
)
$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public class CapWin {
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr lp);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int w, int hh, uint flags);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr wp, IntPtr lp);
    public struct RECT { public int L, T, R, B; }
    delegate bool EnumProc(IntPtr h, IntPtr lp);
    public static List<IntPtr> WindowsOf(uint pid) {
        var list = new List<IntPtr>();
        EnumWindows((h, lp) => {
            uint p; GetWindowThreadProcessId(h, out p);
            if (p == pid && IsWindowVisible(h)) list.Add(h);
            return true;
        }, IntPtr.Zero);
        return list;
    }
    public static bool Key(IntPtr h, uint vk, bool ctrl, bool shift, bool alt) {
        const uint WM_KEYDOWN = 0x0100, WM_KEYUP = 0x0101, WM_SYSKEYDOWN = 0x0104, WM_SYSKEYUP = 0x0105;
        uint down = (ctrl || alt) ? WM_SYSKEYDOWN : WM_KEYDOWN;
        uint up = (ctrl || alt) ? WM_SYSKEYUP : WM_KEYUP;
        if (ctrl) { PostMessage(h, down, (IntPtr)0x11, (IntPtr)0); }
        if (shift) { PostMessage(h, down, (IntPtr)0x10, (IntPtr)0); }
        if (alt) { PostMessage(h, down, (IntPtr)0x12, (IntPtr)0); }
        bool ok = PostMessage(h, down, (IntPtr)vk, (IntPtr)0);
        PostMessage(h, up, (IntPtr)vk, (IntPtr)0);
        if (alt) { PostMessage(h, up, (IntPtr)0x12, (IntPtr)0); }
        if (shift) { PostMessage(h, up, (IntPtr)0x10, (IntPtr)0); }
        if (ctrl) { PostMessage(h, up, (IntPtr)0x11, (IntPtr)0); }
        return ok;
    }
}
'@
Add-Type -AssemblyName System.Drawing

# 键名 → VK：字母全量生成，特殊键按需列出
$vk = @{}
0..25 | ForEach-Object { $vk[[string][char](65 + $_)] = 0x41 + $_ }
0..9 | ForEach-Object { $vk[[string]$_] = 0x30 + $_ }
$vk['backspace'] = 0x08; $vk['tab'] = 0x09; $vk['enter'] = 0x0D; $vk['esc'] = 0x1B
$vk['space'] = 0x20; $vk['pageup'] = 0x21; $vk['pagedown'] = 0x22; $vk['end'] = 0x23
$vk['home'] = 0x24; $vk['left'] = 0x25; $vk['up'] = 0x26; $vk['right'] = 0x27
$vk['down'] = 0x28; $vk['delete'] = 0x2E; $vk['minus'] = 0xBD; $vk['equal'] = 0xBB

$launch = "--working-directory `"$WorkingDir`" $ExtraArgs".Trim()
$proc = Start-Process -FilePath $Exe -ArgumentList $launch -PassThru
Start-Sleep -Seconds $WarmupSec
if ($proc.HasExited) { throw "probe exited immediately" }
$wins = [CapWin]::WindowsOf($proc.Id)
if ($wins.Count -eq 0) { throw "no window" }
$best = $null; $bestW = -1
foreach ($h in $wins) {
    $r = New-Object CapWin+RECT; [void][CapWin]::GetWindowRect($h, [ref]$r)
    if (($r.R - $r.L) -gt $bestW) { $bestW = $r.R - $r.L; $best = $h }
}
$hwnd = $best
if ($WindowW -gt 0 -and $WindowH -gt 0) {
    [void][CapWin]::SetWindowPos($hwnd, [IntPtr](-1), 0, 0, $WindowW, $WindowH, 0x10)
    Start-Sleep -Milliseconds 800
}

function Invoke-Keys([string]$Spec) {
    $parts = $Spec.Split(' ')
    $main = $parts[-1]
    
    $ctrl = $parts -contains 'ctrl'; $shift = $parts -contains 'shift'; $alt = $parts -contains 'alt'
    if (-not $vk.ContainsKey($main.ToLower())) { throw "unknown key: $main" }
    [void][CapWin]::Key($hwnd, $vk[$main.ToLower()], $ctrl, $shift, $alt)
}

function Save-Shot([string]$Name) {
    Start-Sleep -Milliseconds $SettleMs
    $r = New-Object CapWin+RECT; [void][CapWin]::GetWindowRect($hwnd, [ref]$r)
    $w = $r.R - $r.L; $h = $r.B - $r.T
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc()
    [void][CapWin]::PrintWindow($hwnd, $hdc, 2)
    $g.ReleaseHdc($hdc)
    $g.Dispose()
    $bmp.Save("$OutPrefix$Name.png", [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    Write-Host "shot $Name ${w}x${h}"
}

Save-Shot '-00-launch'
$i = 0
foreach ($spec in $Keys) {
    $i++
    Invoke-Keys $spec
    Save-Shot "-$($i.ToString('00'))-$($spec -replace ' ','-')"
}
Write-Host "pid=$($proc.Id)"
