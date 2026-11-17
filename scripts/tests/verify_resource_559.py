"""Build the reviewed Rust modules and switch real Docker CLI contexts mid-sample."""
import http.server
import json
import os
from pathlib import Path
import shutil
import socketserver
import subprocess
import sys
import tempfile
import threading
import tomllib


def run(argv, **kwargs):
    return subprocess.run(argv, check=True, **kwargs)


class DockerAPI(http.server.BaseHTTPRequestHandler):
    def reply(self, body):
        self.send_response(200)
        self.send_header('Api-Version', '1.48')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        if self.command != 'HEAD':
            self.wfile.write(body)

    def do_HEAD(self):
        self.server.requests.append(self.path)
        self.reply(b'')

    def do_GET(self):
        self.server.requests.append(self.path)
        if self.path.split('?')[0].endswith('/containers/json'):
            self.reply(b'[]')
        elif self.path.endswith('/_ping'):
            self.reply(b'OK')
        else:
            self.send_error(404)

    def log_message(self, *args):
        pass


def prepare(source, root):
    code = source / 'pebrel_app/src'
    (root / 'src/platform').mkdir(parents=True)
    for name in ('platform/local_paths.rs', 'platform/process.rs', 'platform/process_output.rs', 'resource_monitor.rs', 'resource_monitor'):
        (root / 'src' / name).symlink_to(code / name)
    versions = {pkg['name']: pkg['version'] for pkg in tomllib.loads((source / 'Cargo.lock').read_text())['package']}
    (root / 'Cargo.toml').write_text('[package]\nname="sampler"\nversion="0.1.0"\nedition="2024"\n[workspace]\n[dependencies]\n'
        + 'pebrel-completions={path=' + json.dumps(str(source / 'pebrel-completions')) + ',default-features=false}\n'
        + 'serde={version="=' + versions['serde'] + '",features=["derive"]}\n'
        + ''.join(f'{name}="={versions[name]}"\n' for name in ('serde_json', 'tempfile', 'sysinfo', 'libc')))
    (root / 'src/main.rs').write_text('''#![allow(dead_code)]
mod platform { pub(crate) mod local_paths; pub(crate) mod process; pub(crate) mod process_output; }
mod resource_monitor;
use resource_monitor::{Collector, Probe, Request};
fn main() {
    let mut collector = Collector::new();
    let sample = collector.sample(Request { docker:true, processes:false }, &Default::default()).unwrap();
    let Probe::Ready(docker) = sample.docker else { panic!("{:?}",sample.docker) };
    assert!(docker.containers.is_empty());
    println!("{}", docker.endpoint);
}
''')
    run(['cargo', 'test', '--manifest-path', root / 'Cargo.toml', '--bin', 'sampler', 'resource_monitor::'])
    run(['cargo', 'build', '--manifest-path', root / 'Cargo.toml', '--bin', 'sampler'])


def exercise(root):
    cli = shutil.which('docker')
    assert cli, 'Docker CLI required'
    env = {k: v for k, v in os.environ.items() if not k.startswith('DOCKER_')}
    env.update(DOCKER_CONFIG=str(root / 'docker-config'), PROBE_DOCKER=cli)
    (root / 'bin').mkdir()
    shim = root / 'bin/docker'
    shim.write_text('''#!/bin/sh
set -eu
if [ "$1" = context ] && [ "$2" = inspect ]; then
    "$PROBE_DOCKER" "$@"
    "$PROBE_DOCKER" context use second >/dev/null 2>&1
else
    exec "$PROBE_DOCKER" "$@"
fi
''')
    shim.chmod(0o755)
    servers = []
    try:
        for name in ('first', 'second'):
            path = str(root / (name + '.sock'))
            server = socketserver.UnixStreamServer(path, DockerAPI)
            server.requests = []
            servers.append(server)
            threading.Thread(target=server.serve_forever, daemon=True).start()
            run([cli, 'context', 'create', name, '--docker', 'host=unix://' + path], env=env)
        run([cli, 'context', 'use', 'first'], env=env)
        sample_env = env | {'PATH': str(root / 'bin') + ':' + env['PATH']}
        output = run([root / 'target/debug/sampler'], env=sample_env, capture_output=True, text=True).stdout.strip()
        assert run([cli, 'context', 'show'], env=env, capture_output=True, text=True).stdout.strip() == 'second'
        assert output == 'unix://' + str(root / 'first.sock'), output
        assert any('/containers/json' in path for path in servers[0].requests), servers[0].requests
        assert not servers[1].requests, servers[1].requests
        print(json.dumps({'reported': 'first', 'selected_after_inspect': 'second', 'daemon_requests': [s.requests for s in servers]}))
    finally:
        for server in servers:
            server.shutdown()
            server.server_close()


def verify_old_cpu(root, old_module):
    # Run the final tests against the earlier collector and its matching filesystem adapter.
    module = root / 'src/resource_monitor.rs'
    module.unlink()
    shutil.copyfile(old_module, module)
    modules = root / 'src/resource_monitor'
    final_modules = modules.resolve()
    modules.unlink()
    modules.mkdir()
    for entry in final_modules.iterdir():
        source = old_module.with_suffix('') / entry.name if entry.name == 'filesystems.rs' else entry
        (modules / entry.name).symlink_to(source)
    for case in ('immediate_sample_omits_cpu_without_postponing_the_next_interval',
                 'new_process_needs_its_own_cpu_baseline'):
        name = 'resource_monitor::tests::' + case
        result = subprocess.run(['cargo', 'test', '--manifest-path', root / 'Cargo.toml',
                                 '--bin', 'sampler', name, '--', '--exact'],
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        print(result.stdout, flush=True)
        assert result.returncode == 101 and ('test ' + name + ' ... FAILED') in result.stdout
        assert '0 passed; 1 failed' in result.stdout and 'panicked at' in result.stdout


if __name__ == '__main__':
    with tempfile.TemporaryDirectory(prefix='pr559-', dir='/tmp') as work:
        root = Path(work)
        prepare(Path(sys.argv[1]).resolve(), root)
        exercise(root)
        verify_old_cpu(root, Path(sys.argv[2]).resolve())
