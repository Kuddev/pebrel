#!/usr/bin/env python3
"""Exercise an isolated Windows product and sample its own memory counters."""

from __future__ import annotations

import argparse
from contextlib import contextmanager
import ctypes
from ctypes import wintypes
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import statistics
import subprocess
import sys
import threading
import time

if __package__ in {None, ""}:
    sys.path.insert(0, os.fspath(Path(__file__).resolve().parent.parent))

from scripts.conformance.harness import RuntimeClient


MIB = 1024 * 1024


def summarize(samples: list[dict], cycles: int, peak_limit: float | None,
              growth_limit: float | None, *, checkpoints: list[dict] | None = None,
              handle_growth_limit: int | None = None, thread_growth_limit: int | None = None) -> dict:
    if cycles < 2 or not samples:
        raise ValueError("at least two complete cycles and memory samples are required")
    for limit in (peak_limit, growth_limit, handle_growth_limit, thread_growth_limit):
        if limit is not None and (not math.isfinite(limit) or limit < 0):
            raise ValueError("budgets must be finite and nonnegative")
    phases = {}
    for phase in ["idle"] + [f"settled-{n}" for n in range(cycles)] + [
        f"tabs-closed-{n}" for n in range(cycles)
    ]:
        rows = [row for row in samples if row["phase"] == phase]
        if not rows:
            raise ValueError(f"missing measurement phase: {phase}")
        phases[phase] = {
            key: statistics.median(row[key] for row in rows[-5:])
            for key in ("private_commit_bytes", "private_working_set_bytes", "working_set_bytes")
        }
    peak = max(row["private_commit_bytes"] for row in samples)
    # Compare equal workloads after warmup. A falling working set cannot mask
    # growth in committed private allocations.
    output_growth = max(phases[f"settled-{n}"]["private_commit_bytes"] for n in range(1, cycles)) - phases["settled-0"]["private_commit_bytes"]
    tab_growth = max(phases[f"tabs-closed-{n}"]["private_commit_bytes"] for n in range(1, cycles)) - phases["tabs-closed-0"]["private_commit_bytes"]
    violations = []
    if peak_limit is not None and peak > peak_limit * MIB:
        violations.append("sampled private commit exceeded the configured peak budget")
    if growth_limit is not None and max(output_growth, tab_growth) > growth_limit * MIB:
        violations.append("retained private commit grew beyond the configured warm-cycle budget")
    resources = {}
    if checkpoints is not None:
        for phase in phases:
            rows = [row for row in checkpoints if row["phase"] == phase]
            if len(rows) != 1:
                raise ValueError(f"require one resource checkpoint for phase: {phase}")
            resources[phase] = {key: rows[0][key] for key in ("handles", "threads")}
    elif handle_growth_limit is not None or thread_growth_limit is not None:
        raise ValueError("resource budgets require complete resource checkpoints")
    resource_growth = {}
    for key, limit in (("handles", handle_growth_limit), ("threads", thread_growth_limit)):
        growth = None
        if resources:
            growth = max(
                resources[f"{prefix}-{n}"][key] - resources[f"{prefix}-0"][key]
                for prefix in ("settled", "tabs-closed") for n in range(1, cycles)
            )
        resource_growth[key] = growth
        if limit is not None and growth > limit:
            violations.append(f"retained {key} grew beyond the configured warm-cycle budget")
    return {
        "phases": phases,
        "resource_checkpoints": resources,
        "resource_growth_after_warmup": resource_growth,
        "sampled_peak_private_commit_bytes": peak,
        "sampled_peak_private_working_set_bytes": max(row["private_working_set_bytes"] for row in samples),
        "output_growth_after_warmup_bytes": output_growth,
        "tab_growth_after_warmup_bytes": tab_growth,
        "peak_budget_mib": peak_limit,
        "growth_budget_mib": growth_limit,
        "handle_growth_budget": handle_growth_limit,
        "thread_growth_budget": thread_growth_limit,
        "budget_enforced": any(limit is not None for limit in (
            peak_limit, growth_limit, handle_growth_limit, thread_growth_limit)),
        "violations": violations,
    }


class NativeProcess:
    class Counters(ctypes.Structure):
        _fields_ = [("size", wintypes.DWORD), ("page_faults", wintypes.DWORD)] + [
            (name, ctypes.c_size_t) for name in (
                "peak_working_set", "working_set", "peak_paged_pool", "paged_pool",
                "peak_nonpaged_pool", "nonpaged_pool", "pagefile", "peak_pagefile",
                "private_usage", "private_working_set", "shared_commit",
            )
        ]

    class SystemMemory(ctypes.Structure):
        _fields_ = [("length", wintypes.DWORD), ("load", wintypes.DWORD)] + [
            (name, ctypes.c_ulonglong) for name in (
                "total_physical", "available_physical", "total_pagefile", "available_pagefile",
                "total_virtual", "available_virtual", "available_extended_virtual",
            )
        ]

    def __init__(self, process: subprocess.Popen, executable: Path):
        self.process = process
        self.kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        self.user = ctypes.WinDLL("user32", use_last_error=True)
        self.kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        self.kernel.OpenProcess.restype = wintypes.HANDLE
        self.kernel.CloseHandle.argtypes = [wintypes.HANDLE]
        self.kernel.K32GetProcessMemoryInfo.argtypes = [wintypes.HANDLE, ctypes.POINTER(self.Counters), wintypes.DWORD]
        self.kernel.QueryFullProcessImageNameW.argtypes = [wintypes.HANDLE, wintypes.DWORD, wintypes.LPWSTR, ctypes.POINTER(wintypes.DWORD)]
        self.kernel.GetProcessTimes.argtypes = [wintypes.HANDLE] + [ctypes.POINTER(wintypes.FILETIME)] * 4
        self.kernel.GetProcessHandleCount.argtypes = [wintypes.HANDLE, ctypes.POINTER(wintypes.DWORD)]
        self.kernel.GlobalMemoryStatusEx.argtypes = [ctypes.POINTER(self.SystemMemory)]
        self.handle = self.kernel.OpenProcess(0x1010, False, process.pid)
        if not self.handle:
            raise ctypes.WinError(ctypes.get_last_error())
        try:
            image = ctypes.create_unicode_buffer(32768)
            length = wintypes.DWORD(len(image))
            if not self.kernel.QueryFullProcessImageNameW(self.handle, 0, image, ctypes.byref(length)):
                raise ctypes.WinError(ctypes.get_last_error())
            if Path(image.value).resolve() != executable.resolve():
                raise RuntimeError("the launched PID does not match the test executable")
        except BaseException:
            self.close()
            raise
        self.user.SetThreadDpiAwarenessContext.argtypes = [wintypes.HANDLE]
        self.user.SetThreadDpiAwarenessContext.restype = wintypes.HANDLE
        self.user.SetThreadDpiAwarenessContext(ctypes.c_void_p(-4))
        self.user.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
        self.user.IsWindowVisible.argtypes = [wintypes.HWND]
        self.user.IsIconic.argtypes = [wintypes.HWND]
        self.user.GetClientRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]
        self.user.GetWindowRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]
        self.user.GetDpiForWindow.argtypes = [wintypes.HWND]
        self.user.SetWindowPos.argtypes = [wintypes.HWND, wintypes.HWND, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int, wintypes.UINT]
        self.user.PostMessageW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]

    def window(self) -> int:
        found = []
        callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)

        @callback_type
        def visit(window, _):
            owner = wintypes.DWORD()
            self.user.GetWindowThreadProcessId(window, ctypes.byref(owner))
            if owner.value == self.process.pid and self.user.IsWindowVisible(window):
                rect = wintypes.RECT()
                self.user.GetClientRect(window, ctypes.byref(rect))
                found.append(((rect.right - rect.left) * (rect.bottom - rect.top), window))
            return True

        self.user.EnumWindows.argtypes = [callback_type, wintypes.LPARAM]
        self.user.EnumWindows(visit, 0)
        if not found:
            raise RuntimeError("the test process has no visible window")
        return max(found)[1]

    def resize(self, width: int, height: int) -> None:
        if not self.user.SetWindowPos(self.window(), None, 0, 0, width, height, 0x16):
            raise ctypes.WinError(ctypes.get_last_error())

    def geometry(self) -> dict:
        window = self.window()
        client, outer = wintypes.RECT(), wintypes.RECT()
        self.user.GetClientRect(window, ctypes.byref(client))
        self.user.GetWindowRect(window, ctypes.byref(outer))
        return {"client_width": client.right - client.left, "client_height": client.bottom - client.top,
                "outer_width": outer.right - outer.left, "outer_height": outer.bottom - outer.top,
                "dpi": self.user.GetDpiForWindow(window), "minimized": bool(self.user.IsIconic(window))}

    def sample(self) -> dict:
        counters = self.Counters()
        counters.size = ctypes.sizeof(counters)
        if not self.kernel.K32GetProcessMemoryInfo(self.handle, ctypes.byref(counters), counters.size):
            raise ctypes.WinError(ctypes.get_last_error())
        created, exited, kernel, user = (wintypes.FILETIME() for _ in range(4))
        if not self.kernel.GetProcessTimes(self.handle, *map(ctypes.byref, (created, exited, kernel, user))):
            raise ctypes.WinError(ctypes.get_last_error())
        ticks = lambda value: value.dwHighDateTime * 2**32 + value.dwLowDateTime
        handles = wintypes.DWORD()
        if not self.kernel.GetProcessHandleCount(self.handle, ctypes.byref(handles)):
            raise ctypes.WinError(ctypes.get_last_error())
        return {"private_commit_bytes": counters.private_usage,
                "private_working_set_bytes": counters.private_working_set,
                "working_set_bytes": counters.working_set,
                "cpu_seconds": (ticks(kernel) + ticks(user)) / 10_000_000,
                "handles": handles.value,
                "created_filetime": ticks(created)}

    def thread_count(self) -> int:
        class ThreadEntry(ctypes.Structure):
            _fields_ = [(name, wintypes.DWORD) for name in ("size", "usage", "thread_id", "process_id")] + [
                ("base_priority", wintypes.LONG), ("delta_priority", wintypes.LONG), ("flags", wintypes.DWORD)
            ]
        self.kernel.CreateToolhelp32Snapshot.argtypes = [wintypes.DWORD, wintypes.DWORD]
        self.kernel.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
        self.kernel.Thread32First.argtypes = [wintypes.HANDLE, ctypes.POINTER(ThreadEntry)]
        self.kernel.Thread32Next.argtypes = [wintypes.HANDLE, ctypes.POINTER(ThreadEntry)]
        snapshot = self.kernel.CreateToolhelp32Snapshot(4, 0)
        if snapshot == ctypes.c_void_p(-1).value:
            raise ctypes.WinError(ctypes.get_last_error())
        count = 0
        try:
            entry = ThreadEntry()
            entry.size = ctypes.sizeof(entry)
            valid = self.kernel.Thread32First(snapshot, ctypes.byref(entry))
            while valid:
                count += entry.process_id == self.process.pid
                valid = self.kernel.Thread32Next(snapshot, ctypes.byref(entry))
            if ctypes.get_last_error() != 18:
                raise ctypes.WinError(ctypes.get_last_error())
            return count
        finally:
            self.kernel.CloseHandle(snapshot)

    def system_memory(self) -> dict:
        status = self.SystemMemory()
        status.length = ctypes.sizeof(status)
        if not self.kernel.GlobalMemoryStatusEx(ctypes.byref(status)):
            raise ctypes.WinError(ctypes.get_last_error())
        return {"physical_total_bytes": status.total_physical,
                "physical_available_bytes": status.available_physical,
                "memory_load_percent": status.load}

    def close(self) -> None:
        if self.handle:
            self.kernel.CloseHandle(self.handle)
            self.handle = None


def close_owned_process(process, native) -> dict:
    """Prefer WM_CLOSE; a forced cleanup is a test failure, never a budget pass."""
    result = {"pid": process.pid, "normal_exit": False, "forced_termination": False, "errors": []}
    if process.poll() is None and native is not None:
        try:
            if not native.user.PostMessageW(native.window(), 0x10, 0, 0):
                raise RuntimeError("WM_CLOSE was rejected")
            process.wait(timeout=15)
        except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
            result["errors"].append(str(error))
    result["normal_exit"] = process.poll() == 0
    if process.poll() is None:
        result["forced_termination"] = True
        try:
            # Popen retains the handle to the exact process we created. Do not
            # search by executable name or touch another instance's children.
            process.terminate()
            process.wait(timeout=15)
        except (OSError, subprocess.TimeoutExpired) as error:
            result["errors"].append(str(error))
    result["returncode"] = process.poll()
    return result


@contextmanager
def isolated_process(executable: Path, work: Path, env: dict, output: Path):
    with (output / "product.log").open("wb") as log:
        process = subprocess.Popen(
            [os.fspath(executable), "--gpui", "--working-directory", os.fspath(work),
             "-e", os.path.join(os.environ["SystemRoot"], "System32", "cmd.exe"), "/d", "/q"],
            cwd=work, env=env, stdout=log, stderr=subprocess.STDOUT,
        )
        native = None
        try:
            native = NativeProcess(process, executable)
            yield process, native
        except BaseException as error:
            (output / "failure.json").write_text(
                json.dumps({"pid": process.pid, "error": str(error)}, indent=2), encoding="utf-8")
            raise
        finally:
            cleanup = close_owned_process(process, native)
            if native is not None:
                native.close()
            (output / "cleanup.json").write_text(json.dumps(cleanup, indent=2), encoding="utf-8")
            report_path = output / "report.json"
            if report_path.exists():
                report = json.loads(report_path.read_text(encoding="utf-8"))
                report["cleanup"] = cleanup
                if not cleanup["normal_exit"]:
                    report["summary"]["violations"].append("the owned test process did not close normally")
                report_path.write_text(json.dumps(report, indent=2), encoding="utf-8")
                if not cleanup["normal_exit"]:
                    raise RuntimeError(f"test process cleanup failed: {cleanup}")


def writer(args: argparse.Namespace) -> None:
    if os.name == "nt":
        import msvcrt
        if not ctypes.windll.kernel32.SetConsoleOutputCP(65001):
            raise ctypes.WinError()
        msvcrt.setmode(sys.stdout.fileno(), os.O_BINARY)
    grid = os.get_terminal_size(sys.stdout.fileno())
    started = time.perf_counter()
    byte_count = 0
    with Path(args.payload).open("rb") as stream:
        while chunk := stream.read(65536):
            remaining = memoryview(chunk)
            while remaining:
                written = os.write(sys.stdout.fileno(), remaining)
                if written <= 0:
                    raise RuntimeError("terminal output stopped accepting bytes")
                byte_count += written
                remaining = remaining[written:]
    elapsed = time.perf_counter() - started
    marker = "PEBREL_STRESS_COMPLETED_" + Path(args.result).stem
    os.write(sys.stdout.fileno(), ("\r\n" + marker + "\r\n").encode())
    Path(args.result).write_text(json.dumps({"bytes": byte_count, "drain_seconds": elapsed, "marker": marker,
                                           "columns": grid.columns, "rows": grid.lines}), encoding="utf-8")


def corpus(directory: Path, mebibytes: int) -> tuple[Path, Path]:
    text = directory / "mixed-output.txt"
    rows = []
    for index in range(512):
        cjk = "".join(chr(0x4E00 + (index * 16 + offset) % 4096) for offset in range(16))
        rows.append(f"\x1b[38;5;{index % 256}m{index:04d} ascii 0123456789 {cjk} \ue0b0 \uf120 \uf07b\x1b[0m\r\n")
    block = "".join(rows).encode("utf-8")
    with text.open("wb") as stream:
        for _ in range((mebibytes * MIB + len(block) - 1) // len(block)):
            stream.write(block)
    redraw = directory / "redraw.txt"
    with redraw.open("wb") as stream:
        stream.write(b"\x1b[?1049h\x1b[?25l")
        for frame in range(240):
            stream.write(b"\x1b[H")
            for row in range(24):
                stream.write((f"\x1b[38;5;{(frame + row) % 256}m" + "▀▄" * 40 + "\x1b[0m\r\n").encode("utf-8"))
        stream.write(b"\x1b[?25h\x1b[?1049l")
    return text, redraw


def run(args: argparse.Namespace) -> int:
    if os.name != "nt":
        raise RuntimeError("this sampler requires native Windows Python")
    executable, output = Path(args.app).resolve(), Path(args.output).resolve()
    output.mkdir(parents=True, exist_ok=False)
    config, work, temporary = (output / name for name in ("config", "work", "temp"))
    for directory in (config, work, temporary):
        directory.mkdir()
    settings = ["shell=cmd", "language=en", "theme=SilverLight", "follow_system_theme=0",
                "font_family=Maple Mono Normal NF CN", "font_size=16", "keep_session=0",
                "restore_session=0", "resume_ai=0", "tray=0", "ai_hooks=0", "ai_toasts=0",
                "fetch=0", "powerline=0", "auto_check_updates=0", "quick_terminal_hotkey=",
                "opacity=1", "blur=0", "windowing_behavior=use_new"]
    (config / "pebrel_settings.txt").write_text("\n".join(settings) + "\n", encoding="utf-8")
    (config / "pebrel.toml").write_text("# Isolated memory stress configuration.\n", encoding="utf-8")
    payloads = corpus(work, args.payload_mib)
    env = {key: value for key, value in os.environ.items() if not key.startswith(("PEBREL_", "NEBULA_", "GPUI_"))}
    for prefix in ("PEBREL", "NEBULA"):
        env[prefix + "_CONFIG_DIR"] = os.fspath(config)
        env[prefix + "_GPUI_CONFIG"] = os.fspath(config / "pebrel.toml")
        env[prefix + "_CONFIG_FILE"] = os.fspath(config / "pebrel.toml")
    env.update(TEMP=os.fspath(temporary), TMP=os.fspath(temporary), PYTHONUTF8="1")
    with isolated_process(executable, work, env, output) as (process, native):
        return measure(args, process, native, executable, output, config, work, payloads)


def measure(args, process, native, executable, output, config, work, payloads) -> int:
    with executable.open("rb") as binary:
        executable_hash = hashlib.file_digest(binary, "sha256").hexdigest()
    source_manifest = None
    if args.source_manifest:
        path = Path(args.source_manifest).resolve()
        source_manifest = {"path": os.fspath(path), "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
    identity = {"pid": process.pid, "executable": os.fspath(executable),
                "sha256": executable_hash,
                "source_manifest": source_manifest,
                "harness_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                "python_version": platform.python_version(),
                "windows_version": platform.version(), "machine": platform.machine(),
                "logical_processors": os.cpu_count(), "system_memory": native.system_memory(),
                **native.sample()}
    (output / "instance.json").write_text(json.dumps(identity, indent=2), encoding="utf-8")
    samples, runs, sampler_errors, checkpoints = [], [], [], []
    stop = threading.Event()
    phase = "startup"
    started = time.monotonic()

    def sample_loop():
        while not stop.is_set():
            try:
                samples.append({"elapsed_seconds": time.monotonic() - started, "phase": phase, **native.sample()})
            except OSError as error:
                sampler_errors.append(str(error))
                return
            stop.wait(0.1)

    sampler = threading.Thread(target=sample_loop, daemon=True)
    sampler.start()
    client = None
    window_id = None

    def checkpoint():
        row = {"phase": phase, "pid": process.pid, "elapsed_seconds": time.monotonic() - started,
               "threads": native.thread_count(), "system_memory": native.system_memory(), **native.sample()}
        checkpoints.append(row)
        print(json.dumps(row), flush=True)
    try:
        deadline = time.monotonic() + 45
        while time.monotonic() < deadline:
            if process.poll() is not None:
                raise RuntimeError(f"the product exited during startup: {process.returncode}")
            try:
                client = RuntimeClient.from_port_file(config / "runtime.port")
                snapshot = client.request("runtime.snapshot")["result"]
                window = snapshot["windows"][0]
                pane_id = window["tabs"][0]["panes"][0]["id"]
                window_id = window["id"]
                if snapshot.get("process_id") != process.pid:
                    raise RuntimeError("runtime endpoint belongs to a different process")
                break
            except (OSError, KeyError, IndexError):
                time.sleep(0.2)
        else:
            raise RuntimeError("the isolated runtime did not become ready")
        native.resize(args.width, args.height)
        time.sleep(5)
        geometry = native.geometry()
        phase = "idle"
        time.sleep(10)
        checkpoint()

        def request(method, params=None):
            return client.request(method, params or {}, timeout=15)["result"]

        def prompt(command):
            return request("pane.prompt", {"window_id": window_id, "pane_id": pane_id, "text": command, "submit": True})

        prompt("chcp 65001 >nul")
        time.sleep(0.5)
        for cycle in range(args.cycles):
            for kind, payload in zip(("output", "redraw"), payloads):
                phase = f"{kind}-{cycle}"
                result_path = work / f"{kind}-{cycle}.json"
                command = subprocess.list2cmdline([sys.executable, "-I", os.fspath(Path(__file__).resolve()),
                                                   "--writer", "--payload", os.fspath(payload), "--result", os.fspath(result_path)])
                prompt(command)
                deadline = time.monotonic() + 120
                while time.monotonic() < deadline:
                    if result_path.is_file():
                        try:
                            result = json.loads(result_path.read_text(encoding="utf-8"))
                        except json.JSONDecodeError:
                            time.sleep(0.1)
                            continue
                        read = request("pane.read", {"window_id": window_id, "pane_id": pane_id, "lines": 80})
                        if result["marker"] in read["text"].splitlines():
                            runs.append({"kind": kind, "cycle": cycle, "history_available": read["history_available"], **result})
                            break
                    time.sleep(0.1)
                else:
                    raise RuntimeError(f"{phase} did not finish in the terminal")
            phase = f"settled-{cycle}"
            time.sleep(args.settle_seconds)
            checkpoint()

        for cycle in range(args.cycles):
            phase = f"tabs-open-{cycle}"
            for _ in range(args.tabs):
                request("tab.new", {"window_id": window_id, "cwd": os.fspath(work)})
            time.sleep(2)
            snapshot = request("runtime.snapshot")
            window = next(value for value in snapshot["windows"] if value["id"] == window_id)
            if len(window["tabs"]) != args.tabs + 1:
                raise RuntimeError("the tab stress did not create the expected live tabs")
            for tab in sorted(window["tabs"], key=lambda value: value["index"], reverse=True):
                if tab["index"] != 0:
                    request("tab.close", {"window_id": window_id, "tab_index": tab["index"]})
            phase = f"tabs-closed-{cycle}"
            time.sleep(args.settle_seconds)
            snapshot = request("runtime.snapshot")
            window = next(value for value in snapshot["windows"] if value["id"] == window_id)
            if len(window["tabs"]) != 1:
                raise RuntimeError("closing the test tabs did not restore one tab")
            checkpoint()
        if sampler_errors:
            raise RuntimeError(f"native sampling failed: {sampler_errors}")
        stop.set()
        sampler.join()
        result = summarize(samples, args.cycles, args.max_private_commit_mib, args.max_growth_mib,
                           checkpoints=checkpoints, handle_growth_limit=args.max_handle_growth,
                           thread_growth_limit=args.max_thread_growth)
        grids = {(row["columns"], row["rows"]) for row in runs}
        if len(grids) != 1:
            raise RuntimeError(f"the workload grid changed during the run: {grids}")
        final_geometry = native.geometry()
        if final_geometry != geometry or final_geometry["minimized"]:
            raise RuntimeError(f"test window geometry changed: {geometry} -> {final_geometry}")
        report = {"identity": identity, "geometry": geometry, "cycles": args.cycles, "tabs_per_cycle": args.tabs,
                  "sampling_interval_seconds": 0.1, "runs": runs, "checkpoints": checkpoints, "summary": result,
                  "notes": ["Counters belong to Pebrel only; shell and writer children are excluded.",
                            "Drain time includes PTY backpressure; it is not displayed FPS or input latency.",
                            "A 100 ms sample interval can miss shorter memory peaks.",
                            "No working-set trimming, process memory limit, or full-memory dump is used."]}
        (output / "report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
        print(json.dumps(result), flush=True)
        return 1 if result["violations"] else 0
    except BaseException:
        context = {"phase": phase, "elapsed_seconds": time.monotonic() - started}
        if client is not None:
            try:
                snapshot = client.request("runtime.snapshot", timeout=2)["result"]
                context["runtime"] = snapshot
                context["pane_processes"] = []
                for window in snapshot["windows"]:
                    if window["id"] != window_id:
                        continue
                    for tab in window["tabs"]:
                        for pane in tab["panes"]:
                            try:
                                tree = client.request("pane.processes", {
                                    "window_id": window_id, "pane_id": pane["id"],
                                }, timeout=2)
                            except (OSError, RuntimeError) as error:
                                tree = {"error": str(error)}
                            context["pane_processes"].append({"pane_id": pane["id"], "tree": tree})
            except (OSError, RuntimeError, KeyError) as error:
                context["diagnostic_error"] = str(error)
        (output / "failure-context.json").write_text(json.dumps(context, indent=2), encoding="utf-8")
        raise
    finally:
        stop.set()
        sampler.join(timeout=2)
        (output / "samples.json").write_text(json.dumps(samples, indent=2), encoding="utf-8")
        (output / "checkpoints.json").write_text(json.dumps(checkpoints, indent=2), encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--writer", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--payload", help=argparse.SUPPRESS)
    parser.add_argument("--result", help=argparse.SUPPRESS)
    parser.add_argument("--app")
    parser.add_argument("--output")
    parser.add_argument("--source-manifest", help="source hashes associated with the test executable")
    parser.add_argument("--cycles", type=int, default=5)
    parser.add_argument("--tabs", type=int, default=4)
    parser.add_argument("--payload-mib", type=int, default=11)
    parser.add_argument("--settle-seconds", type=float, default=3)
    parser.add_argument("--width", type=int, default=1280)
    parser.add_argument("--height", type=int, default=900)
    parser.add_argument("--max-private-commit-mib", type=float)
    parser.add_argument("--max-growth-mib", type=float)
    parser.add_argument("--max-handle-growth", type=int)
    parser.add_argument("--max-thread-growth", type=int)
    args = parser.parse_args()
    if args.writer:
        writer(args)
        return 0
    if not args.app or not args.output:
        parser.error("--app and a fresh --output directory are required")
    if args.cycles < 2 or args.tabs < 1 or args.payload_mib < 1 or not math.isfinite(args.settle_seconds) or args.settle_seconds < 1:
        parser.error("require cycles >= 2, tabs >= 1, payload-mib >= 1, settle-seconds >= 1")
    if min(args.width, args.height) < 200:
        parser.error("window width and height must be at least 200 pixels")
    for limit in (args.max_private_commit_mib, args.max_growth_mib, args.max_handle_growth, args.max_thread_growth):
        if limit is not None and (not math.isfinite(limit) or limit < 0):
            parser.error("budgets must be finite and nonnegative")
    return run(args)


if __name__ == "__main__":
    raise SystemExit(main())
