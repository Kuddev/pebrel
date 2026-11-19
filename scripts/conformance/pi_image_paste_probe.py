"""Exercise Windows clipboard input with an isolated Pebrel, PowerShell 7 and Pi."""
import ctypes
from ctypes import wintypes
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from conformance.harness import ConformanceContext, ResolvedApp


OBSERVER = """
import fs from "node:fs";
import path from "node:path";
export default function(pi) {
  pi.on("session_start", (_event, ctx) => {
    const root = process.env.PEBREL_PASTE_PROBE;
    fs.writeFileSync(path.join(root, "ready"), "ready");
    let last;
    const timer = setInterval(() => {
      let wanted;
      try { wanted = JSON.parse(fs.readFileSync(path.join(root, "expected.json"), "utf8")); }
      catch { return; }
      if (last === wanted || !ctx.ui.getEditorText().includes(path.basename(wanted))) return;
      const bytes = fs.readFileSync(wanted);
      if (bytes.subarray(0, 8).toString("hex") !== "89504e470d0a1a0a") return;
      fs.writeFileSync(path.join(root, "received"), path.basename(wanted));
      last = wanted;
    }, 50);
    timer.unref();
  });
}
"""

SET_CLIPBOARD = """
Add-Type -AssemblyName System.Drawing,System.Windows.Forms
$bitmap = [Drawing.Bitmap]::new(2,2)
try {
  $bitmap.SetPixel(0,0,[Drawing.Color]::FromArgb(255,21,47,89))
  $stream = [IO.MemoryStream]::new()
  $bitmap.Save($stream,[Drawing.Imaging.ImageFormat]::Png)
  $stream.Position = 0
  $data = [Windows.Forms.DataObject]::new()
  $data.SetImage($bitmap)
  $data.SetData('PNG',$false,$stream)
  [Windows.Forms.Clipboard]::SetDataObject($data,$true,10,100)
} finally { $bitmap.Dispose() }
"""

CHECK_PIXELS = """
Add-Type -AssemblyName System.Drawing
$files = @(Get-Content -Raw $env:PEBREL_PASTE_MANIFEST | ConvertFrom-Json)
if ($files.Count -ne 33) { throw 'Expected 33 staged PNGs' }
foreach ($file in $files) {
  $bitmap = [Drawing.Bitmap]::new([string]$file)
  try {
    $pixel = $bitmap.GetPixel(0,0)
    if ($bitmap.Width -ne 2 -or $bitmap.Height -ne 2 -or $pixel.R -ne 21 -or $pixel.G -ne 47 -or $pixel.B -ne 89) {
      throw 'The staged PNG differs from the native clipboard fixture'
    }
  } finally { $bitmap.Dispose() }
}
"""


def powershell(script):
    subprocess.run(["pwsh", "-NoProfile", "-Sta", "-Command", "$ErrorActionPreference='Stop'\n" + script],
                   check=True, timeout=45)


def native_paste(pid):
    user = ctypes.WinDLL("user32", use_last_error=True)
    user.GetForegroundWindow.restype = wintypes.HWND
    user.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
    owner = wintypes.DWORD()
    user.GetWindowThreadProcessId(user.GetForegroundWindow(), ctypes.byref(owner))
    if owner.value != pid:
        raise RuntimeError("The isolated Pebrel window does not own foreground input")
    user.keybd_event.argtypes = [wintypes.BYTE, wintypes.BYTE, wintypes.DWORD, ctypes.c_size_t]
    for key, flags in [(0x11, 0), (0x56, 0), (0x56, 2), (0x11, 2)]:
        user.keybd_event(key, 0, flags, 0)


def run(app, root):
    ctx = ConformanceContext(app, "windows-x86_64", root / "config", root / "project",
                             root / "evidence", startup_timeout=40)
    ctx.prepare()
    with (ctx.config_dir / "pebrel_settings.txt").open("a", encoding="utf-8") as out:
        out.write("shell=pwsh\n")
    observer = root / "observer.mjs"
    observer.write_text(OBSERVER, encoding="utf-8")
    os.environ.update(PEBREL_PASTE_PROBE=str(root), PI_CODING_AGENT_DIR=str(root / "pi-profile"),
                      PI_OFFLINE="1")
    def quote(path):
        return "'" + str(path).replace("'", "''") + "'"
    def paths():
        return set(Path(tempfile.gettempdir()).glob("pebrel-paste-*.png"))
    try:
        ctx.start()
        ctx.refresh_targets()
        ctx.prompt("Write-Output ('PEBREL-PWSH-' + $PSVersionTable.PSVersion.Major)")
        ctx.poll(lambda: "PEBREL-PWSH-7" in ctx.read()["text"], bool, "PowerShell 7 unavailable")
        ctx.prompt("& " + quote(os.environ["PEBREL_FIXTURE_PI"]) + " --no-session -e " + quote(observer))
        ctx.poll((root / "ready").is_file, bool, "Pi editor did not start", timeout=45)
        ctx.api("window.focus", {"window_id": ctx.window_id, "pane_id": ctx.pane_id})
        powershell(SET_CLIPBOARD)
        seen = paths()
        received = []
        for number in range(1, 34):
            native_paste(ctx.process.pid)
            fresh = ctx.poll(lambda: paths() - seen, bool, f"no staged file on paste {number}")
            if len(fresh) != 1:
                raise RuntimeError(f"paste {number} staged {len(fresh)} files")
            path, = fresh
            seen.add(path)
            (root / "expected.json").write_text(json.dumps(str(path)), encoding="utf-8")
            receipt = root / "received"
            ctx.poll(lambda: receipt.exists() and receipt.read_text() == path.name, bool,
                     f"Pi did not receive and open PNG {number}")
            received.append(str(path))
        manifest = root / "manifest.json"
        manifest.write_text(json.dumps(received), encoding="utf-8")
        os.environ["PEBREL_PASTE_MANIFEST"] = str(manifest)
        powershell(CHECK_PIXELS)
        print("33 Ctrl+V events delivered 33 distinct PNGs to one Pi process; all pixels matched.")
    finally:
        ctx.stop(force=True)
        app.close()


if __name__ == "__main__":
    if os.name != "nt" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise SystemExit("Run on a dedicated hosted Windows desktop")
    with tempfile.TemporaryDirectory(prefix="pebrel-clipboard-", ignore_cleanup_errors=True) as directory:
        run(ResolvedApp(sys.argv[1]), Path(directory))
