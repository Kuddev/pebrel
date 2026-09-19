// Execute the shipped bridge, including its bounded native-header reader.
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync, mkdirSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { stripTypeScriptTypes } from 'node:module';
import { fileURLToPath } from 'node:url';
import { test, after } from 'node:test';

const root = new URL('../../', import.meta.url);
const rust = readFileSync(new URL('nebula_app/src/ai_hook/win.rs', root), 'utf8');
const embedded = rust.match(/const PI_EXTENSION_TS: &str = r#"([\s\S]*?)"#;/)?.[1];
assert.ok(embedded, 'use the actual generated bridge');
const events = [];
globalThis.__pebrelBridgeSpawn = (_hook, args) => {
  events.push(JSON.parse(args[1]));
  return { unref() {} };
};
const executable = stripTypeScriptTypes(embedded.replace(
  'import { spawn } from "node:child_process";',
  'const spawn = globalThis.__pebrelBridgeSpawn;',
) + '\nexport { sessionFor };');
const bridge = await import('data:text/javascript;base64,' + Buffer.from(executable).toString('base64'));
mkdirSync(new URL('tmp/', root), { recursive: true });
const directory = mkdtempSync(fileURLToPath(new URL('tmp/pi-bridge-', root)));
after(() => rmSync(directory, { recursive: true }));
const context = (id, file) => ({
  sessionManager: { getSessionId: () => id, getSessionFile: () => file },
});

test('native header replaces filename guessing; process badges are never recovery IDs', () => {
  const file = join(directory, 'timestamp_native-id.jsonl');
  writeFileSync(file, JSON.stringify({ type: 'session', id: 'native-id' }) + '\n');
  assert.deepEqual(bridge.sessionFor(context(undefined, file)), {
    session_id: 'native-id', session_file: file,
  });
  assert.deepEqual(bridge.sessionFor(context(undefined, undefined)), {});
  assert.deepEqual(bridge.sessionFor(context(undefined, file + '.missing')), {});
  assert.deepEqual(bridge.sessionFor({ sessionManager: {
    getSessionId() { throw new Error('closing'); }, getSessionFile: () => file,
  } }), { session_id: 'native-id', session_file: file });
});

test('stale or corrupt file cannot replace direct native identity', () => {
  const file = join(directory, 'mismatch.jsonl');
  writeFileSync(file, JSON.stringify({ type: 'session', id: 'old' }) + '\n');
  assert.deepEqual(bridge.sessionFor(context('current', file)), { session_id: 'current' });
  for (const content of ['invalid\n', '{"type":"message","id":"wrong"}\n', 'x'.repeat(20000)]) {
    writeFileSync(file, content);
    assert.deepEqual(bridge.sessionFor(context(undefined, file)), {});
  }
});

test('session switch retains one process ordering stream and reports identity before turns', async () => {
  const previous = process.env.PEBREL_HOOK_EXE;
  process.env.PEBREL_HOOK_EXE = 'test-hook';
  try {
    const callbacks = new Map();
    bridge.default({ on: (kind, callback) => callbacks.set(kind, callback) });
    await callbacks.get('session_start')({}, context('first'));
    await callbacks.get('session_start')({}, context('second'));
    await callbacks.get('agent_start')({}, context('second'));
    await callbacks.get('agent_end')({}, context('second'));
    assert.deepEqual(events.map(event => event.session_id), ['first', 'second', 'second', 'second']);
    assert.deepEqual(events.map(event => event.kind), ['session-start', 'session-start', 'prompt', 'done']);
    assert.equal(new Set(events.map(event => event.bridge_instance)).size, 1);
    for (let i = 1; i < events.length; i++) {
      assert.ok(BigInt(events[i].bridge_sequence) > BigInt(events[i - 1].bridge_sequence));
      assert.notEqual(events[i].event_id, events[i - 1].event_id);
    }
  } finally {
    if (previous === undefined) delete process.env.PEBREL_HOOK_EXE;
    else process.env.PEBREL_HOOK_EXE = previous;
  }
});
