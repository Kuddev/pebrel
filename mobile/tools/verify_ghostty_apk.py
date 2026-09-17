"""Reject legacy engines and require both native Ghostty/russh Android ABIs."""
from pathlib import Path
import hashlib
import io
import tarfile
import json
import re
import struct
import sys
import zipfile


def verify(path: Path) -> dict:
    libraries = []
    with zipfile.ZipFile(path) as apk:
        names = apk.namelist()
        deployment = apk.read("assets/relay-kit.bin")
        if len(deployment) > 2 * 1024 * 1024 or deployment[:2] != b"\x1f\x8b":
            raise ValueError("Deployment resource must retain its gzip bytes")
        with tarfile.open(fileobj=io.BytesIO(deployment), mode="r:gz") as kit:
            members = kit.getmembers()
            required = {"relay/server.mjs", "relay/init.mjs", "relay/invite.mjs", "relay/tls.mjs",
                        "relay/compose.yaml", "relay/Dockerfile", "relay/package-lock.json",
                        "protocol/bridge-policy.json"}
            if not required.issubset({entry.name for entry in members}):
                raise ValueError("Deployment source kit is incomplete")
            if sum(entry.size for entry in members) > 2 * 1024 * 1024 or any(
                    not entry.isfile() or entry.name.startswith("/") or ".." in Path(entry.name).parts
                    for entry in members):
                raise ValueError("Deployment source kit contains invalid entries")
        dex = [apk.read(name) for name in names if name.endswith(".dex")]
        if not dex:
            raise ValueError("Missing dex in application")
        external_terminal = re.compile(rb"Lcom/[^/]+/(?:terminal/|view/Terminal(?:View|Renderer))")
        if any(external_terminal.search(payload) for payload in dex):
            raise ValueError("External legacy terminal classes remain in application")
        if any(b"Lnet/schmizz/sshj/" in payload for payload in dex):
            raise ValueError("SSHJ classes remain in application")
        if not any(b"Lio/github/kuddev/pebrel/ssh/NativeSsh;" in payload for payload in dex):
            raise ValueError("russh JNI class missing from application")
        if not any(b"Lio/github/kuddev/pebrel/terminal/NativeBridge;" in payload for payload in dex):
            raise ValueError("Ghostty JNI class missing from application")
        for descriptor in (b"Lio/github/kuddev/pebrel/ssh/NativeLink;", b"Lio/github/kuddev/pebrel/voice/NativeWhisper;"):
            if not any(descriptor in payload for payload in dex):
                raise ValueError("Mobile link or voice JNI class missing from application")
        allowed_libraries = {"libpebrel_ghostty.so", "libpebrel_ssh.so", "libpebrel_voice.so", "libandroidx.graphics.path.so"}
        for name in names:
            if name.endswith(".so") and (name.split("/")[-1] not in allowed_libraries or
                    name.split("/")[:2] not in [["lib", "arm64-v8a"], ["lib", "x86_64"]]):
                raise ValueError(f"Unexpected native library in application: {name}")
        for abi, machine in (("arm64-v8a", 183), ("x86_64", 62)):
            for library in ("libpebrel_ghostty.so", "libpebrel_ssh.so", "libpebrel_voice.so"):
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
                if library == "libpebrel_ssh.so" and b"Java_io_github_kuddev_pebrel_ssh_NativeLink_create" not in payload:
                    raise ValueError(f"Native mobile link entry point missing: {name}")
                libraries.append({"abi": abi, "library": library, "bytes": len(payload), "sha256": hashlib.sha256(payload).hexdigest(), "load_alignment": alignments})
        if "assets/licenses/Ghostty/Ghostty-MIT.txt" not in names:
            raise ValueError("Ghostty license missing")
        if "assets/licenses/whisper.cpp.txt" not in names:
            raise ValueError("whisper.cpp license missing")
        russh = json.loads(apk.read("assets/licenses/Russh/BUILD.json"))
        dependencies = json.loads(apk.read("assets/licenses/Russh/DEPENDENCIES.json"))
        if not any(p["name"] == "russh" and p["texts"] for p in dependencies):
            raise ValueError("russh license missing")
        upstream = json.loads(apk.read("assets/licenses/Ghostty-UPSTREAM.json"))
    return {"apk": path.name, "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            "external_terminal_absent": True, "sshj_absent": True, "russh_version": russh["russh"], "ghostty_revision": upstream["revision"], "libraries": libraries}


if __name__ == "__main__":
    print(json.dumps(verify(Path(sys.argv[1])), indent=2))
