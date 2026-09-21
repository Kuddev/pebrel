"""SSH file adapter. Policy and provider configuration are supplied by Rust.

The script is sent over an authenticated exec channel, never typed into the
interactive shell. No credentials or hook tokens are stored by this adapter.
"""
import base64
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

MAX_FILE = 1024 * 1024


def read_file(path):
    if path.is_symlink():
        raise ValueError("integration file is a symbolic link")
    try:
        with path.open("rb") as stream:
            content = stream.read(MAX_FILE + 1)
    except FileNotFoundError:
        return None
    if len(content) > MAX_FILE:
        raise ValueError("integration file exceeds size limit")
    return content


def digest(content):
    return hashlib.sha256(content).hexdigest() if content is not None else None


def capture(program, *args):
    if not program:
        return ""
    try:
        with tempfile.TemporaryFile() as output:
            env = dict(os.environ)
            # Node-managed launchers use /usr/bin/env node. The interpreter
            # beside the discovered launcher must be reachable too.
            env["PATH"] = str(Path(program).parent) + os.pathsep + env.get("PATH", "")
            subprocess.run([program, *args], stdin=subprocess.DEVNULL,
                           stdout=output, stderr=subprocess.DEVNULL,
                           timeout=2, check=True, env=env)
            output.seek(0)
            return output.read(65536).decode("utf-8", "replace")
    except (OSError, subprocess.SubprocessError):
        return ""


def find_program(name):
    found = shutil.which(name)
    if found:
        return found
    home = Path.home()
    candidates = [home / ".local/bin" / name, home / ".npm-global/bin" / name]
    nvm = Path(os.environ.get("NVM_DIR", home / ".nvm"))
    candidates.append(nvm / "current/bin" / name)
    # Noninteractive login shells commonly skip nvm initialization. Probe
    # executable files, not shell output, with a bounded version-directory scan.
    versions = nvm / "versions/node"
    try:
        entries = []
        for i, entry in enumerate(versions.iterdir()):
            if i >= 128:
                break
            version = re.fullmatch(r"v(\d+)\.(\d+)\.(\d+)", entry.name)
            if version:
                entries.append((tuple(map(int, version.groups())), entry))
        candidates.extend(entry / "bin" / name for _, entry in sorted(entries, reverse=True))
    except OSError:
        pass
    return next((str(path) for path in candidates if path.is_file() and os.access(path, os.X_OK)), None)


def locations():
    home = Path.home()
    root = Path(os.environ.get("XDG_DATA_HOME", home / ".local/share")) / "pebrel/ai"
    paths = {
        "claude": Path(os.environ.get("CLAUDE_CONFIG_DIR", home / ".claude")) / "settings.json",
        "codex": Path(os.environ.get("CODEX_HOME", home / ".codex")) / "hooks.json",
        "codex_config": Path(os.environ.get("CODEX_HOME", home / ".codex")) / "config.toml",
        "opencode": Path(os.environ.get("XDG_CONFIG_HOME", home / ".config")) / "opencode/plugins/pebrel.js",
        "pi": home / ".pi/agent/extensions/pebrel.ts",
        "manifest": root / "manifest.json",
        "disabled": root / "disabled",
    }
    for name in ("pebrel-hook", "bridge.py", "shell.py", "bashrc", ".zshenv", ".zprofile", ".zshrc"):
        paths[name] = root / name
    return root, paths


def snapshot():
    if sys.version_info < (3, 8):
        raise ValueError("Python 3.8 or later is required")
    root, paths = locations()
    files = {}
    for name, path in paths.items():
        content = read_file(path)
        files[name] = {
            "path": str(path), "sha256": digest(content),
            "content": content.decode("utf-8") if content is not None else None,
        }
    codex = find_program("codex")
    return {
        "version": 1, "root": str(root), "python": sys.executable,
        "shell": os.environ.get("SHELL", "/bin/sh"), "files": files,
        "providers": {
            "claude": paths["claude"].parent.is_dir(),
            "codex": paths["codex"].parent.is_dir(),
            "opencode": paths["opencode"].parent.parent.is_dir(),
            "pi": paths["pi"].parent.parent.is_dir(),
        },
        "codex_version": capture(codex, "--version") if paths["codex"].parent.is_dir() else "",
        "codex_features": capture(codex, "features", "list") if paths["codex"].parent.is_dir() else "",
    }


def replace(path, content, executable=False):
    path.parent.mkdir(parents=True, exist_ok=True)
    # Refuse symlinked parents as well as the leaf: managed integration files
    # must not overwrite an unrelated file through a redirection.
    if any(parent.is_symlink() for parent in [path, *path.parents]):
        raise ValueError("integration path contains a symbolic link")
    if content is None:
        path.unlink(missing_ok=True)
        return
    fd, temporary = tempfile.mkstemp(prefix=".pebrel-", dir=path.parent)
    try:
        with os.fdopen(fd, "wb") as stream:
            stream.write(content)
            stream.flush()
            os.fsync(stream.fileno())
        mode = path.stat().st_mode & 0o777 if path.exists() else (0o700 if executable else 0o600)
        os.chmod(temporary, mode)
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def apply(plan):
    root, paths = locations()
    root.mkdir(parents=True, exist_ok=True, mode=0o700)
    if any(parent.is_symlink() for parent in [root, *root.parents]):
        raise ValueError("integration root contains a symbolic link")
    lock_fd = os.open(root / ".install.lock", os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    with os.fdopen(lock_fd, "a+b") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        originals = {}
        for item in plan:
            name = item["name"]
            if name not in paths or name in originals:
                raise ValueError("invalid integration file identity")
            content = read_file(paths[name])
            if digest(content) != item["expected"]:
                raise ValueError("integration file changed during setup")
            originals[name] = content
        written = []
        try:
            for item in plan:
                name = item["name"]
                if digest(read_file(paths[name])) != item["expected"]:
                    raise ValueError("integration file changed during setup")
                content = item["content"].encode("utf-8") if item["content"] is not None else None
                if content is not None and len(content) > MAX_FILE:
                    raise ValueError("integration file exceeds size limit")
                if content == originals[name]:
                    continue
                replace(paths[name], content, name == "pebrel-hook")
                written.append((name, digest(content)))
        except Exception:
            for name, expected in reversed(written):
                if digest(read_file(paths[name])) == expected:
                    replace(paths[name], originals[name], name == "pebrel-hook")
            raise
    return {"version": 1, "applied": True}


def run(request):
    try:
        action = request.get("action", "snapshot")
        if action == "snapshot":
            result = snapshot()
        elif action == "apply":
            result = apply(request["files"])
        else:
            raise ValueError("unknown integration action")
    except Exception as error:
        # Do not put config contents, command arguments or secrets in diagnostics.
        result = {"version": 1, "error": type(error).__name__}
    print("PEBREL_INTEGRATION=" + json.dumps(result, separators=(",", ":")))
