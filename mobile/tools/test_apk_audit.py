"""Positive and negative fixtures for native APK composition gates (not real APKs)."""
from pathlib import Path
import json
import struct
import tempfile
import unittest
import zipfile

from verify_ghostty_apk import verify


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
        result = {"classes.dex": b"Lio/github/kuddev/pebrel/terminal/NativeBridge;Lio/github/kuddev/pebrel/ssh/NativeSsh;",
                  "assets/licenses/Ghostty/Ghostty-MIT.txt": b"fixture",
                  "assets/licenses/Ghostty-UPSTREAM.json": b'{"revision":"fixture"}',
                  "assets/licenses/Russh/BUILD.json": b'{"russh":"0.62.2"}',
                  "assets/licenses/Russh/DEPENDENCIES.json": json.dumps([
                      {"name": "russh", "version": "0.62.2", "texts": ["LICENSE"]}]).encode()}
        for abi, machine in (("arm64-v8a", 183), ("x86_64", 62)):
            for library in ("libpebrel_ghostty.so", "libpebrel_ssh.so"):
                result[f"lib/{abi}/{library}"] = elf(machine)
        return result

    def audit(self, contents):
        with tempfile.TemporaryDirectory() as directory:
            apk = Path(directory) / "fixture.apk"
            with zipfile.ZipFile(apk, "w") as archive:
                for name, payload in contents.items():
                    archive.writestr(name, payload)
            return verify(apk)

    def test_accepts_both_engines_and_abis(self):
        self.assertEqual(len(self.audit(self.contents())["libraries"]), 4)

    def test_rejects_legacy_sshj(self):
        contents = self.contents()
        contents["classes.dex"] += b"Lnet/schmizz/sshj/SSHClient;"
        with self.assertRaisesRegex(ValueError, "SSHJ"):
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
