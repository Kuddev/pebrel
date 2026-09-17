"""Verify and restore a public CI dependency artifact for an APK-only preview.

The app and JNI adapter are still compiled from the current checkout. Normal CI
builds compile the VT dependency from source; this opt-in path accepts only a
successful run of this repository's unchanged pinned-core builder.
"""
from pathlib import Path
import argparse
import base64
import hashlib
import json
import os
import re
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[2]


def restore(directory: Path, run_id: str, gh: str, output: Path) -> None:
    repository = os.environ.get('GITHUB_REPOSITORY', 'Kuddev/pebrel')
    if not run_id.isdecimal() or not re.fullmatch(r'[\w.-]+/[\w.-]+', repository):
        raise ValueError('Invalid producer identity')

    def api(endpoint: str):
        return json.loads(subprocess.check_output([gh, 'api', endpoint], text=True, timeout=45))

    run = api(f'repos/{repository}/actions/runs/{run_id}')
    if (run['conclusion'] != 'success' or run['path'] != '.github/workflows/mobile-ghostty.yml' or
            run['head_repository']['full_name'] != repository or run['event'] not in ('push', 'workflow_dispatch')):
        raise ValueError('The dependency must come from a successful same-repository core build')
    revision = run['head_sha']
    if not re.fullmatch(r'[a-f0-9]{40}', revision):
        raise ValueError('Invalid producer revision')
    for name in ('mobile/android/ghostty/UPSTREAM.json', 'mobile/tools/build_ghostty.py',
                 '.github/workflows/mobile-ghostty.yml'):
        content = api(f'repos/{repository}/contents/{name}?ref={revision}')
        if content['encoding'] != 'base64' or base64.b64decode(content['content']) != (ROOT / name).read_bytes():
            raise ValueError(f'Producer inputs differ from this checkout: {name}')
    pins = json.loads((ROOT / 'mobile/android/ghostty/UPSTREAM.json').read_text())
    artifact_root = directory / 'mobile/android/ghostty/build/upstream'
    verified = []
    for abi in ('arm64-v8a', 'x86_64'):
        source = artifact_root / abi
        record = json.loads((source / 'SOURCE.json').read_text())
        if record.get('abi') != abi or any(record.get(key) != value for key, value in pins.items()):
            raise ValueError(f'Pinned source or ABI mismatch: {abi}')
        library = source / 'lib/libghostty-vt.a'
        with library.open('rb') as stream:
            digest = hashlib.file_digest(stream, 'sha256').hexdigest()
        if digest != record.get('library_sha256'):
            raise ValueError(f'Library digest mismatch: {abi}')
        for required in ('include/ghostty/vt.h', 'licenses/Ghostty-MIT.txt', 'licenses/Zig-MIT.txt'):
            if not (source / required).is_file():
                raise ValueError(f'Missing public dependency input: {abi}/{required}')
        if any(path.is_symlink() for path in source.rglob('*')):
            raise ValueError(f'Unexpected symbolic link in dependency artifact: {abi}')
        verified.append((abi, source, digest))
    output.mkdir(parents=True, exist_ok=True)
    for abi, source, digest in verified:
        shutil.copytree(source, output / abi, dirs_exist_ok=True)
        print(f'Verified public core {abi}: {digest}')
    (output / 'CI-PROVENANCE.json').write_text(json.dumps({
        'repository': repository, 'run': run_id, 'source_commit': revision,
        'scope': 'Pinned third-party VT dependency only; app and JNI are rebuilt',
    }, indent=2) + '\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--directory', required=True, type=Path)
    parser.add_argument('--run', required=True)
    parser.add_argument('--gh', default='gh')
    parser.add_argument('--output', type=Path, default=ROOT / 'mobile/android/ghostty/build/upstream')
    args = parser.parse_args()
    restore(args.directory, args.run, args.gh, args.output)
