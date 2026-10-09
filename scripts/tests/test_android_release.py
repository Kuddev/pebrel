from __future__ import annotations

import copy
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from mobile.tools.build_ghostty import build_environment
from scripts import android_release as android
from scripts.preview_release import sha256


class AndroidReleaseTests(unittest.TestCase):
    def test_extracted_terminal_source_cannot_discover_the_application_tag(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            sources = root / "unpacked" / "terminal-core"
            sources.mkdir(parents=True)
            environment = os.environ.copy()
            for name in ("GIT_DIR", "GIT_WORK_TREE", "GIT_COMMON_DIR", "GIT_CEILING_DIRECTORIES"):
                environment.pop(name, None)

            def git(*args, cwd=root, env=environment):
                return subprocess.run(
                    ["git", "-C", str(cwd), *args], env=env,
                    text=True, encoding="utf-8", capture_output=True,
                )

            self.assertEqual(git("init").returncode, 0)
            committed = git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                            "-c", "commit.gpgsign=false", "-c", "core.hooksPath=",
                            "commit", "--allow-empty", "-m", "Application fixture")
            self.assertEqual(committed.returncode, 0, committed.stderr)
            self.assertEqual(git("tag", "v2.2.0").returncode, 0)
            # 失败对照重现上游的真实 Git 查询，证明标签来自父仓库。
            leaked = git("describe", "--exact-match", "--tags", cwd=sources)
            self.assertEqual(leaked.stdout.strip(), "v2.2.0")
            with patch.dict(os.environ, environment, clear=True):
                for inherited in ({}, {"GIT_DIR": str(root / ".git"),
                                       "GIT_WORK_TREE": str(root),
                                       "GIT_COMMON_DIR": str(root / ".git")}):
                    with self.subTest(inherited=inherited), patch.dict(os.environ, inherited):
                        isolated = build_environment(sources, root / "ndk", root / "output")
                        detected = git("rev-parse", "--abbrev-ref", "HEAD", cwd=sources, env=isolated)
                        self.assertNotEqual(detected.returncode, 0)
                        self.assertIn("not a git repository", detected.stderr.lower())
                        self.assertEqual(isolated["ANDROID_NDK_HOME"], str(root / "ndk"))
            self.assertEqual(git("describe", "--exact-match", "--tags").stdout.strip(), "v2.2.0")

    def identity(self):
        metadata = {"applicationId": android.APPLICATION_ID, "variantName": "preview", "elements": [
            {"filters": [], "outputFile": "app-preview.apk", "versionCode": 18, "versionName": "2.0.0-preview"},
        ]}
        badging = f"package: name='{android.APPLICATION_ID}' versionCode='18' versionName='2.0.0-preview' platformBuildVersionName='15'\n"
        signature = f"Signer #1 certificate SHA-256 digest: {android.CERTIFICATE_SHA256}\n"
        return metadata, badging, signature

    def test_accepts_current_preview_and_incremented_version(self):
        self.assertEqual(android.validate_identity(*self.identity(), "2.0.0"), 18)
        self.assertEqual(android.asset_name("2.0.0"), "Pebrel-v2.0.0-android-universal-preview.apk")

    def test_rejects_production_debug_and_split_variants(self):
        metadata, badging, signature = self.identity()
        for field, value in (("applicationId", android.APPLICATION_ID.removesuffix(".preview")),
                             ("variantName", "debug")):
            changed = copy.deepcopy(metadata)
            changed[field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                android.validate_identity(changed, badging, signature, "2.0.0")
        for value in ([{"filterType": "ABI", "value": "arm64-v8a"}], None):
            changed = copy.deepcopy(metadata)
            changed["elements"][0]["filters"] = value
            with self.assertRaises(ValueError):
                android.validate_identity(changed, badging, signature, "2.0.0")
        with self.assertRaisesRegex(ValueError, "debuggable"):
            android.validate_identity(metadata, badging + "application-debuggable\n", signature, "2.0.0")

    def test_rejects_wrong_signer_version_and_compiled_package(self):
        metadata, badging, signature = self.identity()
        for changed in ("", signature.replace(android.CERTIFICATE_SHA256, "a" * 64),
                        signature + signature.replace("#1", "#2")):
            with self.assertRaisesRegex(ValueError, "signer"):
                android.validate_identity(metadata, badging, changed, "2.0.0")
        for code in (0, 17, True, "18"):
            changed = copy.deepcopy(metadata)
            changed["elements"][0]["versionCode"] = code
            with self.assertRaisesRegex(ValueError, "version"):
                android.validate_identity(changed, badging, signature, "2.0.0")
        with self.assertRaisesRegex(ValueError, "version"):
            android.validate_identity(metadata, badging, signature, "2.0.1")
        with self.assertRaisesRegex(ValueError, "Compiled"):
            android.validate_identity(metadata, badging.replace("versionCode='18'", "versionCode='19'"), signature, "2.0.0")

    def test_reports_require_executed_tests_and_reject_failures(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaisesRegex(ValueError, "Missing"):
                android.test_summary(root)
            report = root / "TEST-unit.xml"
            report.write_text('<testsuite failures="0" errors="0"><testcase name="pass"/><testcase name="skip"><skipped/></testcase></testsuite>', encoding="utf-8")
            self.assertEqual(android.test_summary(root), {"passed": 1, "skipped": 1})
            invalid = [
                '<testsuite><testcase><failure/></testcase></testsuite>',
                '<testsuite errors="1"><testcase/></testsuite>',
                '<testsuite><testcase><skipped/></testcase></testsuite>',
                '<testsuite tests="100"/>',
            ]
            for text in invalid:
                report.write_text(text, encoding="utf-8")
                with self.subTest(text=text), self.assertRaises(ValueError):
                    android.test_summary(root)

    def test_aggregate_binds_evidence_to_source_apk_and_both_test_suites(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            name = android.asset_name("2.0.0")
            apk = root / name
            apk.write_bytes(b"APK byte identity fixture, not an installable package")
            data = {"schema_version": 1, "status": "passed", "commit": "a" * 40, "version": "2.0.0",
                    "application_id": android.APPLICATION_ID, "version_code": 18,
                    "certificate_sha256": android.CERTIFICATE_SHA256, "apk": name, "sha256": sha256(apk),
                    "tests": {"unit": {"passed": 1}, "instrumented": {"passed": 1}}}
            report = root / "report.json"
            report.write_text(json.dumps(data), encoding="utf-8")
            android.validate_evidence(report, root, "2.0.0", "a" * 40)
            for field, value in (("commit", "b" * 40), ("sha256", "b" * 64), ("version_code", 17),
                                 ("certificate_sha256", "b" * 64), ("tests", {"unit": {"passed": 1}})):
                invalid = {**data, field: value}
                report.write_text(json.dumps(invalid), encoding="utf-8")
                with self.subTest(field=field), self.assertRaises(ValueError):
                    android.validate_evidence(report, root, "2.0.0", "a" * 40)

    def test_release_workflow_requires_android_and_reuses_preview_signing(self):
        root = Path(__file__).resolve().parents[2]
        workflow = (root / ".github/workflows/release.yml").read_text(encoding="utf-8")
        android_workflow = (root / ".github/workflows/android-release.yml").read_text(encoding="utf-8")
        self.assertIn("uses: ./.github/workflows/android-release.yml", workflow)
        self.assertIn("needs: [prepare, linux, macos, windows, windows-arm64, android]", workflow)
        self.assertIn("release-dist/*.apk", workflow)
        for required in (":app:testDebugUnitTest", ":app:lintPreview", ":app:assemblePreview",
                         ":app:connectedPreviewAndroidTest", "--commit", "stable-package-android",
                         "stable-evidence-android", "aarch64-unknown-linux-musl", "x86_64-unknown-linux-musl"):
            self.assertIn(required, android_workflow)
        self.assertNotIn("continue-on-error", android_workflow)
        self.assertNotIn("assembleDebug", android_workflow)
        self.assertNotIn("keytool -genkey", android_workflow)
        self.assertIn("dist/*-relay-manual.tar.gz", android_workflow)
        self.assertIn("workflow_dispatch:", android_workflow)
        self.assertEqual(android_workflow.count("ref: ${{ env.SOURCE_COMMIT }}"), 2)
        self.assertEqual(android_workflow.count('test "$(git rev-parse HEAD)" = "$SOURCE_COMMIT"'), 2)
        self.assertEqual(android_workflow.count('--commit "$SOURCE_COMMIT"'), 2)
        self.assertNotIn('--commit "$GITHUB_SHA"', android_workflow)
        self.assertIn("GIT_CEILING_DIRECTORIES: ${{ github.workspace }}/mobile/android", android_workflow)

    def test_manual_kit_evidence_is_required_only_after_the_published_211_manifest(self):
        self.assertIsNone(android.manual_asset_name("2.1.1"))
        version, commit = "2.1.2", "a" * 40
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            apk = root / android.asset_name(version)
            apk.write_bytes(b"APK identity fixture")
            kit = root / android.manual_asset_name(version)
            kit.write_bytes(b"manual kit identity fixture")
            data = {"schema_version": 1, "status": "passed", "commit": commit, "version": version,
                    "application_id": android.APPLICATION_ID, "version_code": 21,
                    "certificate_sha256": android.CERTIFICATE_SHA256, "apk": apk.name, "sha256": sha256(apk),
                    "tests": {"unit": {"passed": 1}, "instrumented": {"passed": 1}},
                    "relay_kit": {"file": kit.name, "sha256": sha256(kit)}}
            report = root / "report.json"
            report.write_text(json.dumps(data), encoding="utf-8")
            android.validate_evidence(report, root, version, commit)
            kit.write_bytes(b"changed")
            with self.assertRaisesRegex(ValueError, "Manual relay"):
                android.validate_evidence(report, root, version, commit)


if __name__ == "__main__":
    unittest.main()
