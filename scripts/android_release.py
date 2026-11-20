#!/usr/bin/env python3
"""Verify the existing Preview identity and the APK actually tested for a release."""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET

if __package__:
    from scripts.preview_release import sha256, write_atomic
    from scripts.stable_release import expected_asset_names
else:
    from preview_release import sha256, write_atomic
    from stable_release import expected_asset_names


ROOT = Path(__file__).resolve().parents[1]
APPLICATION_ID = "io.github.kuddev.pebrel.mobile.preview"
# 固定当前预览包的证书，防止 CI 悄悄换成 debug、旧预览或新生成的签名。
CERTIFICATE_SHA256 = "93587953edfe9611601768833b5f008ff700d85c2486ddcfb7f7ea82b2de827b"


def asset_name(version: str) -> str:
    names = [name for name in expected_asset_names(version) if name.endswith(".apk")]
    if len(names) != 1:
        raise ValueError("This release requires exactly one Android Preview APK")
    return names[0]


def manual_asset_name(version: str) -> str | None:
    return next((name for name in expected_asset_names(version) if name.endswith("-relay-manual.tar.gz")), None)


def validate_identity(metadata: dict, badging: str, signature: str, version: str) -> int:
    elements = metadata.get("elements", [])
    if (metadata.get("applicationId") != APPLICATION_ID or metadata.get("variantName") != "preview"
            or len(elements) != 1 or elements[0].get("filters") != []
            or elements[0].get("outputFile") != "app-preview.apk"):
        raise ValueError("Expected the universal Preview APK metadata")
    element = elements[0]
    code = element.get("versionCode")
    # 当前安装基线为 17；本轮发布必须向前递增，不能靠更换文件名伪装升级。
    if type(code) is not int or code <= 17 or element.get("versionName") != f"{version}-preview":
        raise ValueError("Android version name/code does not advance the Preview release")
    package = re.search(r"^package: name='([^']+)' versionCode='([0-9]+)' versionName='([^']+)'", badging, re.M)
    if (not package or package.groups() != (APPLICATION_ID, str(code), f"{version}-preview")
            or re.search(r"^application-debuggable(?:\s|$)", badging, re.M)):
        raise ValueError("Compiled APK identity differs or is debuggable")
    certificates = re.findall(r"^Signer #[0-9]+ certificate SHA-256 digest: ([0-9a-fA-F]{64})$", signature, re.M)
    if [value.lower() for value in certificates] != [CERTIFICATE_SHA256]:
        raise ValueError("APK signer differs from the current Preview certificate")
    return code


def test_summary(directory: Path) -> dict:
    files = sorted(directory.rglob("TEST-*.xml"))
    if not files:
        raise ValueError(f"Missing Android test results: {directory.name}")
    passed = skipped = 0
    for path in files:
        root = ET.parse(path).getroot()
        if root.findall(".//failure") or root.findall(".//error"):
            raise ValueError(f"Android tests failed: {path.name}")
        for suite in root.iter("testsuite"):
            if int(suite.get("failures", "0")) or int(suite.get("errors", "0")):
                raise ValueError(f"Android test suite failed: {path.name}")
        for case in root.iter("testcase"):
            if case.find("skipped") is not None:
                skipped += 1
            else:
                passed += 1
    if not passed:
        raise ValueError("Android test report contains no executed passing tests")
    return {"passed": passed, "skipped": skipped}


def run(*command: str | Path) -> str:
    return subprocess.check_output([str(part) for part in command], text=True, encoding="utf-8", timeout=180)


def package(version: str, commit: str, sdk: Path, output: Path, report: Path) -> None:
    if not re.fullmatch(r"[0-9a-f]{40}", commit) or run("git", "rev-parse", "HEAD").strip() != commit:
        raise ValueError("Android package source must match the release commit")
    app = ROOT / "mobile/android/app/build"
    apk = app / "outputs/apk/preview/app-preview.apk"
    metadata = json.loads(apk.with_name("output-metadata.json").read_text(encoding="utf-8"))
    build_tools = sdk / "build-tools/35.0.0"
    badging = run(build_tools / "aapt", "dump", "badging", apk)
    signature = run(build_tools / "apksigner", "verify", "--verbose", "--print-certs", apk)
    code = validate_identity(metadata, badging, signature, version)
    run(build_tools / "zipalign", "-c", "-P", "16", "4", apk)

    # 复用已有 ABI、JNI、许可证及内置中继校验，不另造第二套内容清单。
    sys.path.insert(0, str(ROOT / "mobile/tools"))
    from verify_ghostty_apk import verify as verify_native
    from verify_native_relay_apk import verify as verify_relay
    native = verify_native(apk)
    verify_relay(apk, commit, require_licenses=True)
    tests = {
        "unit": test_summary(app / "test-results/testDebugUnitTest"),
        "instrumented": test_summary(app / "outputs/androidTest-results/connected"),
    }
    output.mkdir(parents=True, exist_ok=True)
    target = output / asset_name(version)
    if target.exists():
        raise ValueError("Android release output already exists")
    shutil.copyfile(apk, target)
    if sha256(target) != native["sha256"]:
        raise ValueError("APK changed during release collection")
    relay_kit = None
    if name := manual_asset_name(version):
        from package_manual_relay import package as package_manual
        if (output / name).exists():
            raise ValueError("Manual relay release output already exists")
        # 单包校验文件只留在暂存目录，公开集合统一由 SHA256SUMS 管理。
        with tempfile.TemporaryDirectory(prefix="relay-kit-", dir=output.parent) as temporary:
            staging = Path(temporary).resolve()
            if not staging.is_relative_to(output.parent.resolve()):
                raise ValueError("Manual kit staging escaped its output directory")
            package_manual(apk, staging / name, commit)
            shutil.copyfile(staging / name, output / name)
            if sha256(output / name) != sha256(staging / name):
                raise ValueError("Manual relay kit changed during collection")
        relay_kit = {"file": name, "sha256": sha256(output / name)}
    write_atomic(report, json.dumps({
        "schema_version": 1, "status": "passed", "commit": commit,
        "version": version, "application_id": APPLICATION_ID, "version_code": code,
        "certificate_sha256": CERTIFICATE_SHA256, "apk": target.name,
        "sha256": sha256(target), "tests": tests, "native": native, "relay_kit": relay_kit,
    }, indent=2) + "\n")


def validate_evidence(report: Path, directory: Path, version: str, commit: str) -> None:
    data = json.loads(report.read_text(encoding="utf-8"))
    name = asset_name(version)
    expected = {"schema_version": 1, "status": "passed", "commit": commit,
                "version": version, "application_id": APPLICATION_ID, "apk": name,
                "certificate_sha256": CERTIFICATE_SHA256, "sha256": sha256(directory / name)}
    if any(data.get(key) != value for key, value in expected.items()):
        raise ValueError("Android evidence differs from the release APK or source")
    code = data.get("version_code")
    if type(code) is not int or code <= 17:
        raise ValueError("Android evidence does not advance the installed Preview")
    for kind in ("unit", "instrumented"):
        count = data.get("tests", {}).get(kind, {}).get("passed")
        if type(count) is not int or count < 1:
            raise ValueError(f"Missing passed Android {kind} tests")
    if kit := manual_asset_name(version):
        if data.get("relay_kit") != {"file": kit, "sha256": sha256(directory / kit)}:
            raise ValueError("Manual relay evidence differs from the collected archive")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--sdk", type=Path)
    parser.add_argument("--verify-evidence", action="store_true")
    args = parser.parse_args()
    if args.verify_evidence:
        validate_evidence(args.report, args.output, args.version, args.commit)
    elif args.sdk:
        package(args.version, args.commit, args.sdk, args.output, args.report)
    else:
        parser.error("--sdk is required when collecting an APK")


if __name__ == "__main__":
    main()
