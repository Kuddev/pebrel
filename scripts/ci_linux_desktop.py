#!/usr/bin/env python3
"""Exercise the Linux desktop lifecycle on the existing native test binary."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time

from ci_native_tests import native_commands


def run_backend(backend: str, output: Path) -> None:
    output.mkdir(parents=True, exist_ok=False)
    environment = os.environ.copy()
    environment.update(PEBREL_DESKTOP_QA_DIR=str(output),
                       PEBREL_CONFIG_DIR=str(output / 'config'),
                       NEBULA_CONFIG_DIR=str(output / 'config'),
                       LEAK_BACKTRACE='1')
    compositor = None
    compositor_log = None
    try:
        if backend == 'wayland':
            runtime = output / 'runtime'
            runtime.mkdir(mode=0o700)
            environment['XDG_RUNTIME_DIR'] = str(runtime)
            environment['WAYLAND_DISPLAY'] = 'wayland-pebrel-desktop-test'
            compositor_log = (output / 'weston.log').open('w', encoding='utf-8')
            compositor = subprocess.Popen([
                'weston', '--backend=x11-backend.so', '--use-pixman', '--no-config',
                '--socket=' + environment['WAYLAND_DISPLAY'], '--width=1600', '--height=1000',
                '--idle-time=0',
            ], env=environment, stdout=compositor_log, stderr=subprocess.STDOUT)
            deadline = time.monotonic() + 20
            while not (runtime / environment['WAYLAND_DISPLAY']).is_socket():
                if compositor.poll() is not None or time.monotonic() >= deadline:
                    raise RuntimeError('Weston did not create its socket; see weston.log')
                time.sleep(0.05)
            environment['DISPLAY'] = ''
            environment['WAYLAND_DEBUG'] = '1'
        else:
            environment.pop('WAYLAND_DISPLAY', None)
            environment.pop('WAYLAND_DEBUG', None)
        command = native_commands()[2] + [
            'native_linux_silent_start_close_and_restore_keep_the_same_session',
            '--', '--ignored', '--nocapture', '--test-threads=1',
        ]
        log_path = output / 'native.log'
        with log_path.open('w', encoding='utf-8') as log:
            result = subprocess.run(command, env=environment, stdout=log,
                                    stderr=subprocess.STDOUT, timeout=600, check=False)
        if result.returncode:
            print('\n'.join(log_path.read_text(encoding='utf-8', errors='replace').splitlines()[-80:]))
            raise RuntimeError(f'{backend} native fixture failed: {result.returncode}')
        report = json.loads((output / 'result.json').read_text(encoding='utf-8'))
        if report['backend'] != backend:
            raise RuntimeError(f'Expected {backend}, actually tested {report["backend"]}')
        if backend == 'wayland':
            trace = log_path.read_text(encoding='utf-8', errors='replace')
            before, after = trace.split('PEBREL_DESKTOP_QA_SHOW', 1)
            # 核对真实 surface 提交；窗口对象存在不代表静默启动时没有映射。
            attach = rf'wl_surface[@#]{report["id"]}\.attach\(wl_buffer[@#]\d+'
            if re.search(attach, before) or not re.search(attach, after):
                raise RuntimeError('Wayland buffer trace does not prove hidden startup then presentation')
            if 'pebrel-qa-cancelled-activation' in trace:
                raise RuntimeError('Wayland consumed an activation token after a later hide request')
            if 'set_app_id("io.github.kuddev.pebrel")' not in trace:
                raise RuntimeError('Wayland did not publish the matching desktop app_id')
            report['initial_surface_unmapped_then_presented'] = True
            report['cancelled_activation_not_sent'] = True
        (output / 'result.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
        print(json.dumps(report))
    finally:
        if compositor is not None:
            compositor.terminate()
            try:
                compositor.wait(timeout=10)
            except subprocess.TimeoutExpired:
                compositor.kill()
                compositor.wait()
        if compositor_log is not None:
            compositor_log.close()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--backend', choices=('x11', 'wayland'))
    arguments = parser.parse_args()
    if arguments.backend:
        run_backend(arguments.backend, arguments.output.resolve())
    else:
        for backend in ('x11', 'wayland'):
            subprocess.run([
                'xvfb-run', '-a', '-s', '-screen 0 1600x1000x24', sys.executable,
                str(Path(__file__).resolve()), '--backend', backend,
                '--output', str(arguments.output.resolve() / backend),
            ], check=True)


if __name__ == '__main__':
    main()
