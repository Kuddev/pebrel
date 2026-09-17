"""Positive and negative fixtures for native APK composition gates (not real APKs)."""
from pathlib import Path
import json
import io
import tarfile
import struct
import tempfile
import unittest
import zipfile

from verify_ghostty_apk import verify


def deployment_kit() -> bytes:
    output = io.BytesIO()
    with tarfile.open(fileobj=output, mode="w:gz") as archive:
        for name in ("relay/server.mjs", "relay/init.mjs", "relay/invite.mjs", "relay/tls.mjs",
                     "relay/compose.yaml", "relay/Dockerfile", "relay/package-lock.json",
                     "protocol/bridge-policy.json"):
            entry = tarfile.TarInfo(name)
            entry.size = len(b"fixture")
            archive.addfile(entry, io.BytesIO(b"fixture"))
    return output.getvalue()


def elf(machine: int, alignment: int = 16384) -> bytes:
    data = bytearray(128)
    data[:6] = b"\x7fELF\x02\x01"
    struct.pack_into("<H", data, 18, machine)
    struct.pack_into("<Q", data, 32, 64)
    struct.pack_into("<HH", data, 54, 56, 1)
    struct.pack_into("<I", data, 64, 1)
    struct.pack_into("<Q", data, 112, alignment)
    return bytes(data)


class ApkAuditTest(unittest.TestCase):
    def contents(self):
        result = {"assets/relay-kit.bin": deployment_kit(), "classes.dex": b"Lio/github/kuddev/pebrel/terminal/NativeBridge;Lio/github/kuddev/pebrel/ssh/NativeSsh;Lio/github/kuddev/pebrel/ssh/NativeLink;Lio/github/kuddev/pebrel/voice/NativeWhisper;",
                  "assets/licenses/Ghostty/Ghostty-MIT.txt": b"fixture",
                  "assets/licenses/whisper.cpp.txt": b"fixture",
                  "assets/licenses/Ghostty-UPSTREAM.json": b'{"revision":"fixture"}',
                  "assets/licenses/Russh/BUILD.json": b'{"russh":"0.62.2"}',
                  "assets/licenses/Russh/DEPENDENCIES.json": json.dumps([
                      {"name": "russh", "version": "0.62.2", "texts": ["LICENSE"]}]).encode()}
        for abi, machine in (("arm64-v8a", 183), ("x86_64", 62)):
            for library in ("libpebrel_ghostty.so", "libpebrel_ssh.so", "libpebrel_voice.so"):
                result[f"lib/{abi}/{library}"] = elf(machine) + (
                    b"Java_io_github_kuddev_pebrel_ssh_NativeLink_create" if library == "libpebrel_ssh.so" else b"")
        return result

    def audit(self, contents):
        with tempfile.TemporaryDirectory() as directory:
            apk = Path(directory) / "fixture.apk"
            with zipfile.ZipFile(apk, "w") as archive:
                for name, payload in contents.items():
                    archive.writestr(name, payload)
            return verify(apk)

    def test_accepts_both_engines_and_abis(self):
        self.assertEqual(len(self.audit(self.contents())["libraries"]), 6)

    def test_rejects_stale_native_transport_without_secure_entry_point(self):
        contents = self.contents()
        contents["lib/arm64-v8a/libpebrel_ssh.so"] = elf(183)
        with self.assertRaisesRegex(ValueError, "entry point missing"):
            self.audit(contents)

    def test_rejects_missing_voice_abi_or_license(self):
        for name in ("lib/x86_64/libpebrel_voice.so", "assets/licenses/whisper.cpp.txt"):
            contents = self.contents()
            del contents[name]
            with self.assertRaises((ValueError, KeyError)):
                self.audit(contents)

    def test_rejects_transformed_deployment_resource(self):
        contents = self.contents()
        contents["assets/relay-kit.bin"] = b"expanded tar bytes"
        with self.assertRaisesRegex(ValueError, "gzip bytes"):
            self.audit(contents)

    def test_rejects_legacy_sshj(self):
        contents = self.contents()
        contents["classes.dex"] += b"Lnet/schmizz/sshj/SSHClient;"
        with self.assertRaisesRegex(ValueError, "SSHJ"):
            self.audit(contents)

    def test_rejects_external_terminal_classes(self):
        for descriptor in (b"Lcom/legacy/terminal/TerminalEmulator;", b"Lcom/legacy/view/TerminalView;"):
            contents = self.contents()
            contents["classes.dex"] += descriptor
            with self.assertRaisesRegex(ValueError, "External legacy terminal"):
                self.audit(contents)

    def test_rejects_unexpected_native_library(self):
        contents = self.contents()
        contents["lib/arm64-v8a/libexternal-terminal.so"] = elf(183)
        with self.assertRaisesRegex(ValueError, "Unexpected native library"):
            self.audit(contents)

    def test_rejects_missing_transport_abi(self):
        contents = self.contents()
        del contents["lib/arm64-v8a/libpebrel_ssh.so"]
        with self.assertRaises(KeyError):
            self.audit(contents)

    def test_rejects_wrong_abi_and_page_alignment(self):
        for payload in (elf(62), elf(183, 4096)):
            contents = self.contents()
            contents["lib/arm64-v8a/libpebrel_ssh.so"] = payload
            with self.assertRaises(ValueError):
                self.audit(contents)


if __name__ == "__main__":
    unittest.main()
