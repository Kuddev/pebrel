"""Reject legacy engines and require both native Ghostty/russh Android ABIs."""
from pathlib import Path
import hashlib
import json
import struct
import sys
import zipfile


def verify(path: Path) -> dict:
    libraries = []
    with zipfile.ZipFile(path) as apk:
        names = apk.namelist()
        dex = [apk.read(name) for name in names if name.endswith(".dex")]
        if not dex or any(b"Lcom/termux/" in payload for payload in dex):
            raise ValueError("Missing dex or residual Termux classes in application")
        if any(b"Lnet/schmizz/sshj/" in payload for payload in dex):
            raise ValueError("SSHJ classes remain in application")
        if not any(b"Lio/github/kuddev/pebrel/ssh/NativeSsh;" in payload for payload in dex):
            raise ValueError("russh JNI class missing from application")
        if not any(b"Lio/github/kuddev/pebrel/terminal/NativeBridge;" in payload for payload in dex):
            raise ValueError("Ghostty JNI class missing from application")
        if any("libtermux" in name.lower() for name in names):
            raise ValueError("Termux native library remains in application")
        for abi, machine in (("arm64-v8a", 183), ("x86_64", 62)):
            for library in ("libpebrel_ghostty.so", "libpebrel_ssh.so"):
                name = f"lib/{abi}/{library}"
                payload = apk.read(name)
                if payload[:6] != b"\x7fELF\x02\x01" or struct.unpack_from("<H", payload, 18)[0] != machine:
                    raise ValueError(f"Wrong native ABI: {name}")
                offset = struct.unpack_from("<Q", payload, 32)[0]
                stride, count = struct.unpack_from("<HH", payload, 54)
                alignments = []
                for index in range(count):
                    header = offset + index * stride
                    if struct.unpack_from("<I", payload, header)[0] == 1:
                        align = struct.unpack_from("<Q", payload, header + 48)[0]
                        if align < 16384:
                            raise ValueError(f"Native segment does not support 16 KiB pages: {name}")
                        alignments.append(align)
                if not alignments:
                    raise ValueError(f"No loadable native segments: {name}")
                libraries.append({"abi": abi, "library": library, "bytes": len(payload), "sha256": hashlib.sha256(payload).hexdigest(), "load_alignment": alignments})
        if "assets/licenses/Ghostty/Ghostty-MIT.txt" not in names:
            raise ValueError("Ghostty license missing")
        russh = json.loads(apk.read("assets/licenses/Russh/BUILD.json"))
        dependencies = json.loads(apk.read("assets/licenses/Russh/DEPENDENCIES.json"))
        if not any(p["name"] == "russh" and p["texts"] for p in dependencies):
            raise ValueError("russh license missing")
        upstream = json.loads(apk.read("assets/licenses/Ghostty-UPSTREAM.json"))
    return {"apk": path.name, "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            "termux_absent": True, "sshj_absent": True, "russh_version": russh["russh"], "ghostty_revision": upstream["revision"], "libraries": libraries}


if __name__ == "__main__":
    print(json.dumps(verify(Path(sys.argv[1])), indent=2))
