"""Positive and negative fixtures for native APK composition gates (not real APKs)."""
from pathlib import Path
import json
import io
import hashlib
import os
import shutil
import subprocess
import tarfile
import struct
import tempfile
import unittest
import zipfile

from verify_ghostty_apk import verify
from package_manual_relay import package as package_manual
from verify_native_relay_apk import verify as verify_relay
from relay_notices import TARGETS, dependency_packages, read_bundle
from cargo_notices import collect as collect_notices


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


def notice_files(arch, commit):
    files = {"fixture-1/LICENSE": b"MIT license fixture", "Cargo.lock": b"source lock fixture",
             "LICENSE-SOURCE.json": b"{}"}
    manifest = {"schema_version": 1, "commit": commit, "target": TARGETS[arch],
                "packages": [{"name": "fixture", "version": "1", "texts": ["LICENSE"]}],
                "files": {name: hashlib.sha256(data).hexdigest() for name, data in files.items()}}
    return {**files, "manifest.json": json.dumps(manifest).encode()}


class ManualRelayKitTest(unittest.TestCase):
    def test_kit_contains_verified_binaries_and_its_complete_installer(self):
        commit = "a" * 40
        with tempfile.TemporaryDirectory() as directory:
            apk = Path(directory) / "fixture.apk"
            output = Path(directory) / "relay-manual.tar.gz"
            with zipfile.ZipFile(apk, "w") as archive:
                for arch, machine in (("x86_64", 62), ("aarch64", 183)):
                    binary = elf(machine)
                    prefix = f"assets/native-relay/{arch}"
                    archive.writestr(f"{prefix}/pebrel-relay", binary)
                    archive.writestr(f"{prefix}/manifest.json", json.dumps({
                        "protocol": 2, "arch": arch, "commit": commit, "size": len(binary),
                        "sha256": hashlib.sha256(binary).hexdigest(),
                    }))
                    for name, data in notice_files(arch, commit).items():
                        archive.writestr(f"{prefix}/licenses/{name}", data)
            verify_relay(apk, commit, require_licenses=True)
            package_manual(apk, output, commit)
            with tarfile.open(output) as archive:
                root = "pebrel-relay-manual/"
                self.assertEqual(archive.extractfile(root + "SOURCE_COMMIT").read(), (commit + "\n").encode())
                self.assertIn(b"service-install", archive.extractfile(root + "install.sh").read())
                self.assertIn(b"sha256sum -c", archive.extractfile(root + "INSTALL.md").read())
                self.assertIn(b"GNU GENERAL PUBLIC LICENSE", archive.extractfile(root + "LICENSE").read())
                for arch, machine in (("x86_64", 62), ("aarch64", 183)):
                    binary = archive.extractfile(root + arch + "/pebrel-relay").read()
                    self.assertEqual(binary, elf(machine))
                    self.assertEqual(archive.getmember(root + arch + "/pebrel-relay").mode, 0o700)
                    self.assertEqual(archive.extractfile(root + arch + "/licenses/fixture-1/LICENSE").read(),
                                     b"MIT license fixture")
            self.assertEqual(output.with_name(output.name + ".sha256").read_text(encoding="utf-8"),
                             hashlib.sha256(output.read_bytes()).hexdigest() + "  " + output.name + "\n")
            with self.assertRaises(ValueError):
                package_manual(apk, output, commit)
            with self.assertRaises(ValueError):
                package_manual(apk, Path(directory) / "wrong-source.tar.gz", "b" * 40)
            old_apk = Path(directory) / "older.apk"
            with zipfile.ZipFile(apk) as current, zipfile.ZipFile(old_apk, "w") as older:
                for name in current.namelist():
                    if "/licenses/" not in name:
                        older.writestr(name, current.read(name))
            verify_relay(old_apk, commit)
            with self.assertRaises(KeyError):
                verify_relay(old_apk, commit, require_licenses=True)
            with self.assertRaisesRegex(ValueError, "--licenses"):
                package_manual(old_apk, Path(directory) / "missing-notices.tar.gz", commit)
            self.assertFalse((Path(directory) / "missing-notices.tar.gz").exists())
            notices = Path(directory) / "notices"
            for arch in TARGETS:
                for name, data in notice_files(arch, commit).items():
                    path = notices / arch / name
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_bytes(data)
            package_manual(old_apk, Path(directory) / "older-with-notices.tar.gz", commit, notices)

    @unittest.skipUnless(shutil.which("sh"), "requires a POSIX shell for the installer fixture")
    def test_installer_selects_architecture_and_stops_before_unsafe_or_failed_execution(self):
        script = Path(__file__).resolve().parents[1] / "relay-native/install.sh"
        with tempfile.TemporaryDirectory(prefix="pebrel manual kit ") as directory:
            root = Path(directory)
            shutil.copyfile(script, root / "install.sh")
            tools = root / "tools"
            tools.mkdir()
            log = root / "calls"
            stubs = {
                "uname": '#!/bin/sh\ncase "$1" in -s) echo Linux;; -m) echo "$TEST_ARCH";; esac\n',
                "id": '#!/bin/sh\necho 0\n',
            }
            for name, text in stubs.items():
                path = tools / name
                path.write_text(text, encoding="utf-8", newline="\n")
                path.chmod(0o755)
            binary = (b'#!/bin/sh\nprintf "%s\\n" "$@" >> "$TEST_CALLS"\n'
                      b'if [ "$1" = service-install ] && [ "$TEST_FAIL" = 1 ]; then exit 9; fi\n')
            digest = hashlib.sha256(binary).hexdigest()
            for arch in ("x86_64", "aarch64"):
                folder = root / arch
                folder.mkdir()
                (folder / "pebrel-relay").write_bytes(binary)
                (folder / "SHA256SUMS").write_text(digest + "  pebrel-relay\n", encoding="utf-8", newline="\n")

            def run(arch="x86_64", port="18443", fail="0"):
                # PATH 只替换系统事实，安装目标始终是本测试的记录脚本。
                return subprocess.run(
                    [shutil.which("sh"), "-c", 'tools=$(CDPATH= cd -- "$1" && pwd); PATH="$tools:$PATH"; export PATH; exec sh "$2" "$3" "$4"',
                     "fixture", tools.as_posix(), (root / "install.sh").as_posix(), "127.0.0.1", port],
                    env={**os.environ, "TEST_ARCH": arch, "TEST_CALLS": log.as_posix(), "TEST_FAIL": fail},
                    capture_output=True, text=True, encoding="utf-8", timeout=10,
                )

            for arch in ("x86_64", "aarch64", "arm64"):
                result = run(arch)
                self.assertEqual(result.returncode, 0, result.stderr)
                calls = log.read_text(encoding="utf-8").splitlines()
                self.assertEqual(calls[0], "service-install")
                self.assertTrue(calls[2].endswith("/" + ("aarch64" if arch == "arm64" else arch) + "/pebrel-relay"))
                self.assertEqual(calls[3:], ["--sha256", digest, "--address", "127.0.0.1", "--port", "18443", "service-status"])
                log.unlink()
            for arch, port in (("riscv64", "443"), ("x86_64", "0"), ("x86_64", "65536"), ("x86_64", "1;echo")):
                self.assertNotEqual(run(arch, port).returncode, 0)
                self.assertFalse(log.exists())
            failed = run(fail="1")
            self.assertEqual(failed.returncode, 9)
            self.assertNotIn("4/4", failed.stdout)
            self.assertNotIn("service-status", log.read_text(encoding="utf-8"))
            log.unlink()
            with (root / "x86_64/pebrel-relay").open("ab") as stream:
                stream.write(b"changed")
            self.assertNotEqual(run().returncode, 0)
            self.assertFalse(log.exists())


class RelayNoticeTest(unittest.TestCase):
    def test_inventory_binds_texts_to_source_target_and_safe_paths(self):
        commit = "a" * 40
        files = notice_files("x86_64", commit)
        self.assertEqual(read_bundle(files.__getitem__, commit, "x86_64"), files)
        for source, arch in (("b" * 40, "x86_64"), (commit, "aarch64")):
            with self.assertRaises(ValueError):
                read_bundle(files.__getitem__, source, arch)
        changed = dict(files, **{"fixture-1/LICENSE": b"changed"})
        with self.assertRaises(ValueError):
            read_bundle(changed.__getitem__, commit, "x86_64")
        missing = dict(files)
        del missing["fixture-1/LICENSE"]
        with self.assertRaises(KeyError):
            read_bundle(missing.__getitem__, commit, "x86_64")
        for text in ("../secret", "/absolute", "bad\\path"):
            changed = dict(files)
            manifest = json.loads(files["manifest.json"])
            manifest["packages"][0]["texts"] = [text]
            changed["manifest.json"] = json.dumps(manifest).encode()
            with self.assertRaises(ValueError):
                read_bundle(changed.__getitem__, commit, "x86_64")

    def test_target_tree_uses_only_listed_checksum_matched_registry_archives(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive = root / "registry/cache/fixture/fixture-1.0.0.crate"
            archive.parent.mkdir(parents=True)
            manifest = b'[package]\nname="fixture"\nversion="1.0.0"\nlicense="MIT"\nauthors=["fixture author"]\n'
            with tarfile.open(archive, "w:gz") as tar:
                for name, data in (("Cargo.toml", manifest), ("LICENSE", b"license fixture")):
                    info = tarfile.TarInfo("fixture-1.0.0/" + name)
                    info.size = len(data)
                    tar.addfile(info, io.BytesIO(data))
            entry = {"name": "fixture", "version": "1.0.0", "source": "registry+https://example.invalid/index",
                     "checksum": hashlib.sha256(archive.read_bytes()).hexdigest()}
            lock = {"package": [entry, {"name": "unrelated", "version": "1.0.0"}]}
            tree = "pebrel-mobile-link v0.1.0 (/source/mobile/link)\nfixture v1.0.0 (proc-macro)\nfixture v1.0.0 (proc-macro)\n"
            packages = dependency_packages(tree, lock, root, root / "sources")
            self.assertEqual([(p["name"], p["version"]) for p in packages], [("fixture", "1.0.0")])
            self.assertEqual((Path(packages[0]["manifest_path"]).parent / "LICENSE").read_bytes(), b"license fixture")
            with self.assertRaisesRegex(ValueError, "checksum-matched"):
                dependency_packages(tree, {"package": [dict(entry, checksum="0" * 64)]}, root, root / "bad")
            with self.assertRaisesRegex(ValueError, "registry dependency"):
                dependency_packages("fixture v1.0.0 (https://example.invalid/source)", lock, root, root / "unsupported")

    def test_shared_collector_retains_texts_and_selects_only_allowed_fallback(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source"
            source.mkdir()
            (source / "Cargo.toml").write_text("[package]\n", encoding="utf-8")
            (source / "LICENSE").write_text("license fixture", encoding="utf-8")
            apache = root / "Apache.txt"
            apache.write_text("pinned Apache fixture", encoding="utf-8")
            sha = hashlib.sha256(apache.read_bytes()).hexdigest()
            package = {"name": "fixture", "version": "1", "source": "registry",
                       "manifest_path": str(source / "Cargo.toml"), "license": "MIT OR Apache-2.0",
                       "authors": ["fixture author"], "repository": None}
            records = collect_notices([package], root / "output", apache, sha)
            self.assertEqual(records[0]["texts"], ["LICENSE"])
            self.assertIsNone(records[0]["selected_license"])
            (source / "LICENSE").unlink()
            records = collect_notices([package], root / "fallback", apache, sha)
            self.assertEqual(records[0]["selected_license"], "Apache-2.0")
            self.assertIn("upstream-Cargo.toml", records[0]["texts"])
            legacy = collect_notices([dict(package, license="MIT/Apache-2.0")], root / "legacy", apache, sha)
            self.assertEqual(legacy[0]["selected_license"], "Apache-2.0")
            for license in ("MIT", "MIT AND Apache-2.0", "MIT AND (BSD-3-Clause OR Apache-2.0)",
                            "https://example.invalid/Apache-2.0"):
                with self.assertRaises(ValueError):
                    collect_notices([dict(package, license=license)], root / "invalid", apache, sha)
            with self.assertRaises(ValueError):
                collect_notices([package], root / "bad-hash", apache, "0" * 64)


if __name__ == "__main__":
    unittest.main()
