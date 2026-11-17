"""Run native subagent cases, then restore the scheduling-dependent fixture setup."""
from pathlib import Path
import json
import os
import subprocess

source = os.environ['SOURCE']
assert subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip() == source
out = Path('tmp/eviction-fixture-evidence')
out.mkdir(parents=True, exist_ok=True)
results = {'source': source, 'repetitions': []}
for attempt in range(1, 4):
    log = out / f'fixed-{attempt}.log'
    with log.open('w', encoding='utf-8') as stream:
        subprocess.run(['cargo', 'nextest', 'run', '--locked', '--config', '.github/ci-profile.toml',
                        '--cargo-profile', 'ci', '-p', 'pebrel', '--bin', 'pebrel',
                        '--features', 'gpui-test-support', '--test-threads', '1',
                        '--no-fail-fast', '--retries', '0', '-E',
                        'test(ai_hook::native_events::subagent_tests::)'],
                       stdout=stream, stderr=subprocess.STDOUT, check=True, timeout=1800)
    summary = [line for line in log.read_text(encoding='utf-8').splitlines() if 'Summary [' in line]
    assert summary, 'missing native test result'
    results['repetitions'].append(summary)
    print(attempt, summary, flush=True)

path = Path('pebrel_app/src/ai_hook/native_events/subagent_tests.rs')
original = path.read_text(encoding='utf-8')
needle = '''            remember_antigravity_conversation(
                brain.join(&filler_id),
                CachedConversation::NonSystemStepZero,
            );'''
assert original.count(needle) == 1
restored = '''            let filler_transcript = brain.join(&filler_id)
                .join(".system_generated/logs/transcript_full.jsonl");
            assert!(send("session-start", &filler_id, &filler_transcript).is_some());'''
try:
    path.write_text(original.replace(needle, restored), encoding='utf-8')
    (out / 'restored-setup.patch').write_text(
        subprocess.check_output(['git', 'diff', '--', str(path)], text=True), encoding='utf-8')
    with (out / 'restored-setup.log').open('w', encoding='utf-8') as stream:
        result = subprocess.run(['cargo', 'test', '--locked', '--config', '.github/ci-profile.toml',
                                 '--profile', 'ci', '-p', 'pebrel', '--bin', 'pebrel',
                                 '--features', 'gpui-test-support',
                                 'ai_hook::native_events::subagent_tests::antigravity_forked_subagents_are_rejected_on_cold_start_and_after_parent_eviction',
                                 '--', '--exact', '--test-threads=1'],
                                stdout=stream, stderr=subprocess.STDOUT, timeout=1800)
    text = (out / 'restored-setup.log').read_text(encoding='utf-8')
    assert result.returncode == 101
    assert 'assertion failed: send("session-start", &filler_id, &filler_transcript).is_some()' in text
    assert 'test result: FAILED. 0 passed; 1 failed;' in text
    results['restored_setup'] = 'expected filler admission assertion failure with paused worker'
finally:
    path.write_text(original, encoding='utf-8')
(out / 'result.json').write_text(json.dumps(results, indent=2) + '\n', encoding='utf-8')
print(json.dumps(results, indent=2))
