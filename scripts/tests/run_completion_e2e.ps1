# Run product-handler/native-PTY completion acceptance on a desktop that is never switched to.
param(
    [Parameter(Mandatory)][string]$TestBinary,
    [Parameter(Mandatory)][string]$OutputDirectory,
    [ValidateSet('git', 'editor')][string]$Case = 'git',
    [string]$PowerShellProgram,
    [switch]$Prediction
)
$ErrorActionPreference = 'Stop'
$TestBinary = (Resolve-Path -LiteralPath $TestBinary).Path
if (-not [IO.Path]::IsPathRooted($OutputDirectory)) { throw 'OutputDirectory must be absolute' }
if ((Test-Path -LiteralPath $OutputDirectory) -and (Get-ChildItem -LiteralPath $OutputDirectory -Force)) {
    throw 'Use an empty output directory to keep each acceptance result independent'
}
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$OutputDirectory = (Resolve-Path -LiteralPath $OutputDirectory).Path
$env:PEBREL_COMPLETION_QA_DIR = $OutputDirectory
$env:PEBREL_CONFIG_DIR = Join-Path $OutputDirectory 'config'
$env:NEBULA_CONFIG_DIR = $env:PEBREL_CONFIG_DIR
Remove-Item Env:PSModulePath -ErrorAction SilentlyContinue
if ($PowerShellProgram) { $env:PEBREL_COMPLETION_QA_PWSH = (Resolve-Path -LiteralPath $PowerShellProgram).Path.Replace('\', '/') }
else { Remove-Item Env:PEBREL_COMPLETION_QA_PWSH -ErrorAction SilentlyContinue }
if ($Prediction) {
    if (-not $PowerShellProgram) { throw 'Prediction acceptance requires a supplied PowerShell 7 executable' }
    $env:PEBREL_COMPLETION_QA_PREDICTION = '1'
} else { Remove-Item Env:PEBREL_COMPLETION_QA_PREDICTION -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force -Path $env:PEBREL_CONFIG_DIR | Out-Null
$executable = Join-Path $OutputDirectory 'pebrel-test.exe'
# The owned copy prevents concurrent builds from linking over a running test image.
Copy-Item -LiteralPath $TestBinary -Destination $executable
$filter = if ($Case -eq 'git') { 'git_completion_native_shell_end_to_end' } else { 'editor_completion_native_shell_end_to_end' }
$arguments = "gpui_shell::terminal::view::completion_native_tests::$filter --exact --ignored --test-threads=1 --nocapture"
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class PebrelCompletionQaDesktop {
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)]
    public struct Startup {
        public int cb; public string reserved; public string desktop; public string title;
        public uint x,y,xSize,ySize,xCount,yCount,fill,flags; public ushort show,reserved2;
        public IntPtr reservedPtr,input,output,error;
    }
    [StructLayout(LayoutKind.Sequential)] public struct Process { public IntPtr process,thread; public uint pid,tid; }
    [StructLayout(LayoutKind.Sequential)] public struct Security { public int length; public IntPtr descriptor; public int inherit; }
    [StructLayout(LayoutKind.Sequential)] public struct Point { public int x,y; }
    [DllImport("user32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern IntPtr CreateDesktop(string name, IntPtr device, IntPtr mode, uint flags, uint access, IntPtr security);
    [DllImport("user32.dll")] static extern bool CloseDesktop(IntPtr desktop);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool GetCursorPos(out Point point);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern bool CreateProcess(string app, StringBuilder command, IntPtr pa, IntPtr ta, bool inherit, uint flags, IntPtr env, string cwd, ref Startup startup, out Process process);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern IntPtr CreateFile(string path, uint access, uint share, ref Security security, uint creation, uint attrs, IntPtr template);
    [DllImport("kernel32.dll")] static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
    [DllImport("kernel32.dll")] static extern bool GetExitCodeProcess(IntPtr handle, out uint code);
    [DllImport("kernel32.dll")] static extern bool TerminateProcess(IntPtr handle, uint code);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    public static uint Run(string exe, string args, string cwd, string log) {
        string name = "PebrelCompletionQa_" + Guid.NewGuid().ToString("N");
        IntPtr desktop = CreateDesktop(name, IntPtr.Zero, IntPtr.Zero, 0, 0x1ff, IntPtr.Zero);
        if (desktop == IntPtr.Zero) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
        var security = new Security { length = Marshal.SizeOf<Security>(), inherit = 1 };
        IntPtr output = CreateFile(log, 0x40000000, 1, ref security, 2, 0x80, IntPtr.Zero);
        if (output == new IntPtr(-1)) { CloseDesktop(desktop); throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error()); }
        var startup = new Startup { cb = Marshal.SizeOf<Startup>(), desktop = "winsta0\\" + name, flags = 0x100, output = output, error = output };
        Process process;
        try {
            if (!CreateProcess(exe, new StringBuilder("\"" + exe + "\" " + args), IntPtr.Zero, IntPtr.Zero, true, 0x08000000, IntPtr.Zero, cwd, ref startup, out process)) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
            try {
                Console.WriteLine("OWNED_QA_PID=" + process.pid + " DESKTOP=" + name);
                if (WaitForSingleObject(process.process, 600000) != 0) { TerminateProcess(process.process, 124); throw new TimeoutException("Owned completion QA exceeded ten minutes"); }
                uint code; GetExitCodeProcess(process.process, out code); return code;
            } finally { CloseHandle(process.thread); CloseHandle(process.process); }
        } finally { CloseHandle(output); CloseDesktop(desktop); }
    }
}
'@
$beforeWindow = [PebrelCompletionQaDesktop]::GetForegroundWindow().ToInt64()
$beforePoint = [PebrelCompletionQaDesktop+Point]::new()
[void][PebrelCompletionQaDesktop]::GetCursorPos([ref]$beforePoint)
$workingDirectory = $OutputDirectory
try {
    $code = [PebrelCompletionQaDesktop]::Run($executable, $arguments, $workingDirectory, (Join-Path $OutputDirectory 'native.log'))
    $afterPoint = [PebrelCompletionQaDesktop+Point]::new()
    [void][PebrelCompletionQaDesktop]::GetCursorPos([ref]$afterPoint)
    @{
        exit_code=$code
        foreground_before=$beforeWindow
        foreground_after=[PebrelCompletionQaDesktop]::GetForegroundWindow().ToInt64()
        cursor_before=@($beforePoint.x,$beforePoint.y)
        cursor_after=@($afterPoint.x,$afterPoint.y)
        desktop_was_switched=$false
        global_input_injected=$false
        input_path='product EntityInputHandler and keyboard handler'
        case=$Case
        prediction_enabled=[bool]$Prediction
    } | ConvertTo-Json -Depth 4 | Set-Content -Encoding utf8 (Join-Path $OutputDirectory 'desktop-result.json')
    Get-Content (Join-Path $OutputDirectory 'native.log') -Tail 18
} finally { Remove-Item -LiteralPath $executable -ErrorAction SilentlyContinue }
exit $code
