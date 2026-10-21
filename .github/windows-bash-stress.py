"""Temporary diagnostic branch: preserve every original result and wire trace."""
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import argparse
import hashlib
import json
import os
import re
import subprocess
import time

TEST = "tty::windows::test::bash_input::git_bash_wrapped_input_can_be_fully_erased_in_byte_and_utf8_locales"
ARMS = ("native", "zero", "zero-padded")
ROOT = Path("target/bash-diagnostics")
HOSTS = {
    "in-box": "conpty api ready (in-box)",
    "sideloaded": "conpty api ready (sideloaded OpenConsole)",
}


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def compiled_binary():
    candidates = set()
    for line in (ROOT / "cargo-messages.jsonl").read_text(encoding="utf-8-sig").splitlines():
        item = json.loads(line)
        if (item.get("reason") == "compiler-artifact"
                and item.get("target", {}).get("name") == "nebula_terminal"
                and item.get("profile", {}).get("test")
                and item.get("executable")):
            candidates.add(Path(item["executable"]).resolve())
    if len(candidates) != 1:
        raise RuntimeError(f"Expected one Cargo-reported test executable, found {candidates}")
    return candidates.pop()


def inspect_evidence(output, runtime, arm, passed):
    errors = []
    selected_hosts = [host for host, trace in HOSTS.items() if trace in output]
    if selected_hosts != [runtime]:
        errors.append(f"host selection mismatch: {selected_hosts}")
    edits = []
    modes = set()
    for match in re.finditer(r"\[bash-edit\] (\{[^\r\n]*\})", output):
        event = json.loads(match[1])
        wire = bytes(event.pop("bytes"))
        event["sha256"] = hashlib.sha256(wire).hexdigest()
        vk = event["vk"]
        if event.get("arm") != arm:
            errors.append("edit did not record the requested diagnostic arm")
        if event.get("win32") is True:
            modes.add("win32")
            mapped = event["mapped_scan"]
            expected_scan = {"native": str(mapped), "zero": "0", "zero-padded": "00"}[arm]
            character, modifiers = {8: (8, 0), 65: (1, 8), 69: (5, 8), 85: (21, 8), 76: (12, 8)}[vk]
            pair = (f"\x1b[{vk};{expected_scan};{character};1;{modifiers};1_"
                    f"\x1b[{vk};{expected_scan};{character};0;{modifiers};1_").encode()
            if (mapped == 0 or event["scan_field"] != expected_scan
                    or wire != pair * event["count"] or len(wire) != event["byte_count"]):
                errors.append("recorded bytes do not match the arm and original burst count")
        elif event.get("win32") is False:
            modes.add("legacy")
            legacy = {8: 127, 65: 1, 69: 5, 85: 21, 76: 12}[vk]
            if (runtime != "in-box" or wire != bytes([legacy]) * event["count"]
                    or len(wire) != event["byte_count"]):
                errors.append("legacy bytes or host do not match the fallback contract")
        else:
            errors.append("missing explicit negotiated-input mode")
        edits.append(event)
    if not edits:
        errors.append("no edit trace captured")
    if len(modes) > 1:
        errors.append("input mode changed within the scan-code comparison")
    if passed:
        if "test result: ok. 1 passed; 0 failed;" not in output:
            errors.append("the exact regression did not report one passing test")
        for locale in ("C", "C.UTF-8"):
            bursts = [e for e in edits if e["locale"] == locale and e["vk"] == 8 and e["count"] == 72]
            if len(bursts) != 1:
                errors.append(f"missing unique original 72-key burst for {locale}")
    comparison = "invalid"
    if not errors:
        comparison = "not_applicable" if modes == {"legacy"} else "applied"
    return {"hosts": selected_hosts, "edits": edits, "errors": errors,
            "scan_comparison": comparison}


def run_one(binary, runtime, arm, iteration, timeout):
    directory = ROOT / runtime / arm
    log_path = directory / f"attempt-{iteration:03}.log"
    result = {"iteration": iteration, "runtime": runtime, "arm": arm,
              "returncode": None, "timed_out": False, "errors": []}
    environment = os.environ.copy()
    environment.update(NEBULA_BOOT_TRACE="1", PEBREL_BASH_DIAGNOSTIC_SCAN_CODE=arm)
    command = [str(binary), TEST, "--exact", "--nocapture"]
    result["command"] = command
    started = time.monotonic()
    try:
        with log_path.open("wb") as output:
            process = subprocess.Popen(command, stdout=output, stderr=subprocess.STDOUT, env=environment)
            try:
                process.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                result["timed_out"] = True
                try:
                    if os.name == "nt":
                        subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"],
                                       stdout=output, stderr=subprocess.STDOUT, timeout=15, check=False)
                finally:
                    if process.poll() is None:
                        process.kill()
                    process.wait(timeout=15)
            result["returncode"] = process.returncode
        output = log_path.read_bytes().decode("utf-8", errors="replace")
        evidence = inspect_evidence(output, runtime, arm, result["returncode"] == 0)
        result.update(evidence)
    except Exception as error:
        result["errors"].append(f"{type(error).__name__}: {error}")
    result["elapsed_seconds"] = round(time.monotonic() - started, 3)
    result["log_sha256"] = sha256(log_path) if log_path.exists() else None
    result["functional_passed"] = result["returncode"] == 0 and not result["timed_out"]
    result["passed"] = result["functional_passed"] and not result["errors"]
    write_json(directory / f"attempt-{iteration:03}.json", result)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("runtime", choices=HOSTS)
    parser.add_argument("--repetitions", type=int, default=48)
    parser.add_argument("--workers", type=int, default=4)
    parser.add_argument("--timeout", type=int, default=90)
    arguments = parser.parse_args()
    if not 1 <= arguments.repetitions <= 48 or not 1 <= arguments.workers <= 4 or not 1 <= arguments.timeout <= 90:
        parser.error("diagnostic bounds are 1..48 attempts, 1..4 workers and 1..90 seconds")
    environment = json.loads((ROOT / "environment.json").read_text(encoding="utf-8-sig"))
    runtime_files = json.loads((ROOT / f"runtime-{arguments.runtime}.json").read_text(encoding="utf-8-sig"))
    binary = compiled_binary()
    metadata = {"branch": environment["branch"], "head": environment["head"],
                "runtime": arguments.runtime, "msys": environment["msys"],
                "binary": str(binary), "binary_sha256": sha256(binary),
                "runtime_files": runtime_files, "repetitions": arguments.repetitions,
                "workers": arguments.workers, "attempt_timeout_seconds": arguments.timeout}
    (ROOT / arguments.runtime).mkdir(parents=True, exist_ok=True)
    write_json(ROOT / arguments.runtime / "metadata.json", metadata)
    summaries = []
    for arm in ARMS:
        (ROOT / arguments.runtime / arm).mkdir(exist_ok=True)
        # Arms and runtimes are sequential; only attempts within an arm overlap.
        with ThreadPoolExecutor(max_workers=arguments.workers) as executor:
            results = list(executor.map(
                lambda iteration: run_one(binary, arguments.runtime, arm, iteration, arguments.timeout),
                range(1, arguments.repetitions + 1),
            ))
        comparisons = {r.get("scan_comparison", "invalid") for r in results}
        summary = {"branch": metadata["branch"], "head": metadata["head"],
                   "runtime": arguments.runtime, "msys": metadata["msys"], "arm": arm,
                   "attempts": len(results), "workers": arguments.workers,
                   "passed": sum(r["passed"] for r in results),
                   "functional_passed": sum(r["functional_passed"] for r in results),
                   "scan_comparison": comparisons.pop() if len(comparisons) == 1 else "mixed",
                   "test_failures": sum(r["returncode"] != 0 for r in results),
                   "invalid_evidence": sum(bool(r["errors"]) for r in results),
                   "timeouts": sum(r["timed_out"] for r in results),
                   "backspace72_byte_counts": sorted({e["byte_count"] for r in results
                                                     for e in r.get("edits", [])
                                                     if e["vk"] == 8 and e["count"] == 72})}
        summaries.append(summary)
        write_json(ROOT / arguments.runtime / arm / "summary.json", summary)
        print(json.dumps(summary, ensure_ascii=False), flush=True)
    write_json(ROOT / arguments.runtime / "summary.json", summaries)
    return int(any(s["passed"] != arguments.repetitions for s in summaries))


if __name__ == "__main__":
    raise SystemExit(main())
