"""Verify PNG admission and replay the former bounds against the same fixture."""
from pathlib import Path
import json
import os
import subprocess

root = Path('tmp/graphics-reader-evidence')
root.mkdir(parents=True, exist_ok=True)
source = os.environ['SOURCE']
assert subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip() == source
profile = ['--locked', '--config', '.github/ci-profile.toml', '--profile', 'ci']
app = ['cargo', 'test', *profile, '-p', 'pebrel', '--bin', 'pebrel', '--features', 'gpui-test-support']
core = ['cargo', 'test', *profile, '-p', 'pebrel_terminal', '--lib']
target = 'gpui_shell::terminal::inline_image::tests::kitty_png_crosses_the_owned_queue_limit_and_reaches_the_decoded_cache'
results = {'source': source, 'positive': [], 'counterexamples': []}


def execute(label, command, expected_error=None):
    path = root / (label + '.log')
    with path.open('w', encoding='utf-8') as stream:
        result = subprocess.run(command, stdout=stream, stderr=subprocess.STDOUT, timeout=1800)
    text = path.read_text(encoding='utf-8')
    summaries = [line for line in text.splitlines() if 'test result:' in line]
    print(label, result.returncode, summaries, flush=True)
    if expected_error is None:
        assert result.returncode == 0, label
        assert summaries and any('test result: ok.' in s for s in summaries), label
        results['positive'].append({'case': label, 'summaries': summaries})
    else:
        assert result.returncode == 101 and expected_error in text, label
        assert 'test result: FAILED. 0 passed; 1 failed;' in text, label
        results['counterexamples'].append({'case': label, 'expected_error': expected_error,
                                           'summaries': summaries})


execute('inline-images', [*app, 'gpui_shell::terminal::inline_image::tests', '--', '--test-threads=1'])
execute('png-core', [*core, 'png', '--', '--test-threads=1'])
path = Path('pebrel_app/src/gpui_shell/terminal/inline_image.rs')
original = path.read_text(encoding='utf-8')
try:
    needle = 'let Some(owner) = data.upgrade() else { return Ok(()) };'
    assert original.count(needle) == 1
    patch = original.replace(needle, needle + '''
        if placement.format == TransferFormat::Png && owner.len() > MAX_QUEUED_BYTES {
            return Err("terminal image decode queue exceeds 32 MiB");
        }
''')
    path.write_text(patch, encoding='utf-8')
    (root / 'restored-queue-bound.patch').write_text(
        subprocess.check_output(['git', 'diff', '--', str(path)], text=True), encoding='utf-8')
    execute('restored-queue-bound', [*app, target, '--', '--exact', '--test-threads=1'],
            'terminal image decode queue exceeds 32 MiB')
    import re
    patch, count = re.subn(r'(ImageFormat::Png,\s+)pebrel_terminal::kitty_graphics::MAX_TRANSFER_BYTES',
                          r'\1MAX_IMAGE_ENCODED_BYTES', original)
    assert count == 1
    path.write_text(patch, encoding='utf-8')
    (root / 'restored-decoder-bound.patch').write_text(
        subprocess.check_output(['git', 'diff', '--', str(path)], text=True), encoding='utf-8')
    execute('restored-decoder-bound', [*app, target, '--', '--exact', '--test-threads=1'],
            'encoded image exceeds its size limit')
finally:
    path.write_text(original, encoding='utf-8')
(root / 'result.json').write_text(json.dumps(results, indent=2) + '\n', encoding='utf-8')
