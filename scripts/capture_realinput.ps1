# C1-C12 实机验收驱动（SendInput 版）：把探针窗口带到前台后发送真实键盘
# 输入（GPUI 的和弦不认 PostMessage，必须走全局输入队列），每步可截图存证；
# 脚本结束自动结束自己拉起的探针进程（仅按本进程 PID，绝不按映像名）。
# 用法：
#   powershell -File scripts/capture_realinput.ps1 -Exe <exe> -OutPrefix <tag> `
#     -WorkingDir <dir> -ExtraArgs '-e cmd /k' `
#     -Steps @('key:ctrl-shift-o','type:chemistry-rendering-test.md','key:enter',`
#              'shot:open','key:pagedown','shot:scroll1')
param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [Parameter(Mandatory = $true)][string]$OutPrefix,
    [string[]]$Steps = @(),
    [int]$SettleMs = 900,
    [string]$WorkingDir = '',
    [string]$ExtraArgs = '-e cmd /k',
    [int]$WarmupSec = 8,
    [int]$WindowW = 1518,
    [int]$WindowH = 844
)
$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public class RealWin {
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr lp);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int w, int hh, uint flags);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, IntPtr pid);
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool attach);
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    public static bool Focus(IntPtr h) {
        uint target = GetWindowThreadProcessId(h, IntPtr.Zero);
        uint mine = GetCurrentThreadId();
        IntPtr fg = GetForegroundWindow();
        uint foreground = fg == IntPtr.Zero ? 0 : GetWindowThreadProcessId(fg, IntPtr.Zero);
        bool attachedA = AttachThreadInput(mine, target, true);
        bool attachedB = foreground != 0 && foreground != mine && AttachThreadInput(mine, foreground, true);
        bool ok = SetForegroundWindow(h);
        if (attachedA) { AttachThreadInput(mine, target, false); }
        if (attachedB) { AttachThreadInput(mine, foreground, false); }
        return ok;
    }
    [StructLayout(LayoutKind.Sequential)]
    public struct INPUT { public uint type; public InputUnion U; }
    [StructLayout(LayoutKind.Explicit)]
    public struct InputUnion { [FieldOffset(0)] public KEYBDINPUT ki; [FieldOffset(0)] public long pad; }
    [StructLayout(LayoutKind.Sequential)]
    public struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    public struct RECT { public int L, T, R, B; }
    delegate bool EnumProc(IntPtr h, IntPtr lp);
    [DllImport("user32.dll", SetLastError = true)]
    static extern uint SendInput(uint count, INPUT[] inputs, int size);
    public static List<IntPtr> WindowsOf(uint pid) {
        var list = new List<IntPtr>();
        EnumWindows((h, lp) => {
            uint p; GetWindowThreadProcessId(h, out p);
            if (p == pid && IsWindowVisible(h)) list.Add(h);
            return true;
        }, IntPtr.Zero);
        return list;
    }
    static void Send(ushort vk, bool up) {
        var input = new INPUT { type = 1 };
        input.U.ki.wVk = vk;
        input.U.ki.dwFlags = up ? 2u : 0u;
        SendInput(1, new INPUT[] { input }, Marshal.SizeOf(typeof(INPUT)));
    }
    public static void Chord(ushort vk, bool ctrl, bool shift, bool alt) {
        if (ctrl) Send(0x11, false);
        if (shift) Send(0x10, false);
        if (alt) Send(0x12, false);
        Send(vk, false); Send(vk, true);
        if (alt) Send(0x12, true);
        if (shift) Send(0x10, true);
        if (ctrl) Send(0x11, true);
    }
    public static void Unicode(string text) {
        foreach (char ch in text) {
            var down = new INPUT { type = 1 };
            down.U.ki.wVk = 0; down.U.ki.wScan = ch; down.U.ki.dwFlags = 4; // UNICODE
            var up = new INPUT { type = 1 };
            up.U.ki.wVk = 0; up.U.ki.wScan = ch; up.U.ki.dwFlags = 4 | 2;
            SendInput(1, new INPUT[] { down }, Marshal.SizeOf(typeof(INPUT)));
            SendInput(1, new INPUT[] { up }, Marshal.SizeOf(typeof(INPUT)));
        }
    }
}
'@
Add-Type -AssemblyName System.Drawing

$vk = @{}
0..25 | ForEach-Object { $vk[[string][char](65 + $_)] = 0x41 + $_ }
0..9 | ForEach-Object { $vk[[string]$_] = 0x30 + $_ }
$vk['backspace'] = 0x08; $vk['tab'] = 0x09; $vk['enter'] = 0x0D; $vk['esc'] = 0x1B
$vk['space'] = 0x20; $vk['pageup'] = 0x21; $vk['pagedown'] = 0x22; $vk['end'] = 0x23
$vk['home'] = 0x24; $vk['left'] = 0x25; $vk['up'] = 0x26; $vk['right'] = 0x27
$vk['down'] = 0x28; $vk['delete'] = 0x2E; $vk['f5'] = 0x74

$launch = "--working-directory `"$WorkingDir`" $ExtraArgs".Trim()
$proc = Start-Process -FilePath $Exe -ArgumentList $launch -PassThru
try {
    Start-Sleep -Seconds $WarmupSec
    if ($proc.HasExited) { throw "probe exited immediately" }
    $wins = [RealWin]::WindowsOf($proc.Id)
    if ($wins.Count -eq 0) { throw "no window" }
    $best = $null; $bestW = -1
    foreach ($h in $wins) {
        $r = New-Object RealWin+RECT; [void][RealWin]::GetWindowRect($h, [ref]$r)
        if (($r.R - $r.L) -gt $bestW) { $bestW = $r.R - $r.L; $best = $h }
    }
    $hwnd = $best
    [void][RealWin]::ShowWindow($hwnd, 9) # SW_RESTORE
    [void][RealWin]::SetWindowPos($hwnd, [IntPtr](-1), 0, 0, $WindowW, $WindowH, 0x10)
    Start-Sleep -Milliseconds 800

    function Save-Shot([string]$Name) {
        Start-Sleep -Milliseconds $SettleMs
        $r = New-Object RealWin+RECT; [void][RealWin]::GetWindowRect($hwnd, [ref]$r)
        $w = $r.R - $r.L; $h = $r.B - $r.T
        $bmp = New-Object System.Drawing.Bitmap($w, $h)
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $hdc = $g.GetHdc()
        [void][RealWin]::PrintWindow($hwnd, $hdc, 2)
        $g.ReleaseHdc($hdc)
        $g.Dispose()
        $bmp.Save("$OutPrefix$Name.png", [System.Drawing.Imaging.ImageFormat]::Png)
        $bmp.Dispose()
        Write-Host "shot $Name ${w}x${h}"
    }

    foreach ($step in $Steps) {
        $kind, $rest = $step.Split(':', 2)
        switch ($kind) {
            'key' {
                [void][RealWin]::Focus($hwnd)
                Start-Sleep -Milliseconds 220
                $parts = $rest.Split(' '); $main = $parts[-1]
                $ctrl = $parts -contains 'ctrl'; $shift = $parts -contains 'shift'; $alt = $parts -contains 'alt'
                if (-not $vk.ContainsKey($main.ToLower())) { throw "unknown key: $main" }
                [RealWin]::Chord($vk[$main.ToLower()], $ctrl, $shift, $alt)
                Start-Sleep -Milliseconds 260
            }
            'type' {
                [void][RealWin]::Focus($hwnd)
                Start-Sleep -Milliseconds 220
                [RealWin]::Unicode($rest)
                Start-Sleep -Milliseconds 260
            }
            'shot' { Save-Shot "-$rest" }
            'wait' { Start-Sleep -Milliseconds ([int]$rest) }
            default { throw "unknown step kind: $kind" }
        }
    }
    Save-Shot '-zz-final'
} finally {
    if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
}
Write-Host "done"
