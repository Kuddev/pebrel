"""Add the same readout to the baseline and candidate tray window on the hosted runner."""
import os
from pathlib import Path
import subprocess
import sys

if os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
    raise SystemExit("Requires an isolated GitHub-hosted desktop")
path = Path("nebula_app/src/tray.rs")
variant = sys.argv[1]
assert variant in {"baseline", "candidate"}
ref = "v2.6.1" if variant == "baseline" else "HEAD"
source = subprocess.check_output(["git", "show", f"{ref}:{path.as_posix()}"]).decode()
anchor = "        match msg {\n            WM_APP_TRAY =>"
assert source.count(anchor) == 1
probe = """        if msg == 0x804e {
            use windows_sys::Win32::UI::Shell::{NIM_MODIFY, Shell_NotifyIconW};
            let data = notify_data(hwnd);
            // No flags or fields change. Explorer returns success only for a registered icon.
            let registered = unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) } != 0;
            let added = ICONS.with(|slot| slot.borrow().as_ref().is_some_and(|icon| icon.added));
            return 8 | (isize::from(state().enabled) << 2) | (isize::from(added) << 1) | isize::from(registered);
        }
"""
path.write_text(source.replace(anchor,probe + anchor), encoding="utf-8")
subprocess.run(["cargo", "build", "--locked", "--config", ".github/ci-profile.toml", "--profile",
                "ci", "-p", "nebula", "--bin", "pebrel", "--features", "gpui-shell"], check=True)
