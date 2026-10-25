"""Read a bounded metadata catalog; never import or execute project code."""
import base64
import json
import os
import pathlib
import time

config = json.loads(base64.b64decode(INPUT))
deadline = time.monotonic() + 2
remaining = config.get('remaining', 4 * 1024 * 1024)


def read(path, limit=1024 * 1024):
    global remaining
    if time.monotonic() >= deadline:
        raise RuntimeError('deadline')
    with path.open('rb') as stream:
        value = stream.read(min(limit, remaining) + 1)
    if len(value) > min(limit, remaining):
        raise RuntimeError('file budget')
    remaining -= len(value)
    return value


def manifest(path):
    source = json.loads(read(path))
    result = {key: source[key] for key in ('name', 'workspaces') if key in source}
    scripts = source.get('scripts', {})
    result['scripts'] = {key: '' for key, value in scripts.items() if not key.startswith('-') and isinstance(value, str)} if isinstance(scripts, dict) else {}
    for key in ('dependencies', 'devDependencies', 'optionalDependencies'):
        dependencies = source.get(key, {})
        result[key] = {name: '*' for name in dependencies} if isinstance(dependencies, dict) else {}
    return result


root = pathlib.Path(config['cwd'])
for directory in config.get('directories', [])[-1:]:
    root = root / directory
if not root.is_absolute():
    raise RuntimeError('absolute guest cwd required')

if config['mode'] == 'root':
    nearest = None
    for _ in range(64):
        path = root / 'package.json'
        if path.exists():
            data = manifest(path)
            yaml = root / 'pnpm-workspace.yaml'
            pnpm = config.get('manager') == 'pnpm'
            workspace = yaml.exists() if pnpm else isinstance(data.get('workspaces'), (list, dict))
            if not config['workspace'] or workspace:
                yaml_text = read(yaml, 65536).decode() if yaml.exists() else None
                print(json.dumps({'root': str(root), 'manifest': data, 'yaml': yaml_text, 'remaining': remaining}))
                break
            if pnpm and nearest is None:
                nearest = {'root': str(root), 'manifest': data, 'yaml': None}
        if root.parent == root or (root / 'node_modules').is_dir():
            if nearest is not None:
                nearest['remaining'] = remaining
                print(json.dumps(nearest))
                break
            raise RuntimeError('workspace unavailable')
        root = root.parent
    else:
        raise RuntimeError('ancestor budget')
else:
    stack = [root]
    packages = []
    visited = 0
    prefixes = config['prefixes']
    while stack:
        if time.monotonic() >= deadline or visited >= 4096 or len(packages) >= 512:
            raise RuntimeError('workspace budget')
        directory = stack.pop()
        visited += 1
        children = []
        with os.scandir(directory) as entries:
            for entry in entries:
                if len(children) >= 4096 or time.monotonic() >= deadline:
                    raise RuntimeError('directory budget')
                if not entry.is_dir(follow_symlinks=False) or entry.name in ('.git', 'node_modules', 'target', 'dist', 'build'):
                    continue
                child = pathlib.Path(entry.path)
                relative = child.relative_to(root).as_posix()
                if not any(not prefix or relative == prefix or relative.startswith(prefix + '/') or prefix.startswith(relative + '/') for prefix in prefixes):
                    continue
                children.append(child)
        for child in sorted(children, reverse=True):
            path = child / 'package.json'
            if path.exists():
                packages.append({'path': child.relative_to(root).as_posix(), 'manifest': manifest(path)})
            stack.append(child)
    print(json.dumps(packages))
