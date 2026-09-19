// Execute the shipped bridge, including its bounded native-header reader.
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync, mkdirSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { stripTypeScriptTypes } from 'node:module';
import { fileURLToPath } from 'node:url';
import { test, after } from 'node:test';

const root = new URL('../../', import.meta.url);
const bridgeSource = new URL('nebula_app/src/ai_hook/bridges.rs', root);
const bridgePath = readFileSync(bridgeSource, 'utf8').match(/PI_EXTENSION_TS: &str = include_str!\("([^"]+)"\)/)?.[1];
assert.ok(bridgePath, 'test must execute the production PI_EXTENSION_TS asset');
const embedded = readFileSync(new URL(bridgePath, bridgeSource), 'utf8');
const events = [];
globalThis.__pebrelBridgeSpawn = (_hook, args) => {
  events.push(JSON.parse(args[1]));
  return { unref() {} };
};
globalThis.__pebrelPiVersion = '0.85.1';
const executable = stripTypeScriptTypes(embedded.replace(
  'import { spawn } from "node:child_process";',
  'const spawn = globalThis.__pebrelBridgeSpawn;',
).replace(
  'import { VERSION } from "@earendil-works/pi-coding-agent";',
  'const VERSION = globalThis.__pebrelPiVersion;',
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

test('a retryable failed attempt must not report successful completion', async () => {
  const previous = process.env.PEBREL_HOOK_EXE;
  process.env.PEBREL_HOOK_EXE = 'test-hook';
  const firstEvent = events.length;
  try {
    const callbacks = new Map();
    bridge.default({ on: (kind, callback) => callbacks.set(kind, callback) });
    const ctx = { ...context('retrying'), isIdle: () => false };
    await callbacks.get('agent_start')({}, ctx);
    await callbacks.get('agent_end')({ messages: [
      { role: 'assistant', stopReason: 'error', errorMessage: 'synthetic retryable failure' },
    ] }, ctx);
    assert.deepEqual(events.slice(firstEvent).map(event => event.kind), ['prompt']);
  } finally {
    events.splice(firstEvent);
    if (previous === undefined) delete process.env.PEBREL_HOOK_EXE;
    else process.env.PEBREL_HOOK_EXE = previous;
  }
});

for (const reason of ['stop', 'error', 'aborted', 'future', 'length', 'toolUse']) {
  test(`settled ${reason} reports its stop_reason once, without leaking error text`, async () => {
    const previous = process.env.PEBREL_HOOK_EXE;
    process.env.PEBREL_HOOK_EXE = 'test-hook';
    const firstEvent = events.length;
    try {
      const callbacks = new Map();
      bridge.default({ on: (kind, callback) => callbacks.set(kind, callback) });
      const ctx = context('settled');
      await callbacks.get('agent_start')({}, ctx);
      await callbacks.get('agent_end')({ messages: [
        { role: 'assistant', stopReason: reason, errorMessage: 'SECRET-provider-request' },
      ] }, ctx);
      assert.deepEqual(events.slice(firstEvent).map(event => event.kind), ['prompt']);
      assert.equal(typeof callbacks.get('agent_settled'), 'function');
      await callbacks.get('agent_settled')({}, ctx);
      await callbacks.get('agent_settled')({}, ctx);
      assert.deepEqual(events.slice(firstEvent).map(event => event.kind), ['prompt', 'done']);
      assert.equal(events.at(-1).stop_reason, reason);
      assert.equal('outcome' in events.at(-1), false);
      assert.ok(!JSON.stringify(events.slice(firstEvent)).includes('SECRET'));
    } finally {
      events.splice(firstEvent);
      if (previous === undefined) delete process.env.PEBREL_HOOK_EXE;
      else process.env.PEBREL_HOOK_EXE = previous;
    }
  });
}

test('retry success replaces the failed attempt; switching sessions drops unfinished results', async () => {
  const previous = process.env.PEBREL_HOOK_EXE;
  process.env.PEBREL_HOOK_EXE = 'test-hook';
  const firstEvent = events.length;
  try {
    const callbacks = new Map();
    bridge.default({ on: (kind, callback) => callbacks.set(kind, callback) });
    const ctx = context('retry-success');
    await callbacks.get('agent_start')({}, ctx);
    await callbacks.get('agent_end')({ messages: [{ role: 'assistant', stopReason: 'error' }] }, ctx);
    await callbacks.get('agent_start')({}, ctx);
    await callbacks.get('agent_end')({ messages: [{ role: 'assistant', stopReason: 'stop' }] }, ctx);
    await callbacks.get('agent_settled')({}, ctx);
    assert.equal(events.slice(firstEvent).filter(event => event.kind === 'done').length, 1);
    assert.equal(events.at(-1).stop_reason, 'stop');
    await callbacks.get('agent_start')({}, ctx);
    await callbacks.get('agent_end')({ messages: [{ role: 'assistant', stopReason: 'error' }] }, ctx);
    await callbacks.get('session_start')({}, context('new-session'));
    await callbacks.get('agent_settled')({}, context('new-session'));
    assert.equal(events.at(-1).kind, 'session-start');
  } finally {
    events.splice(firstEvent);
    if (previous === undefined) delete process.env.PEBREL_HOOK_EXE;
    else process.env.PEBREL_HOOK_EXE = previous;
  }
});

test('pre-settled Pi versions retain agent_end fallback without registering unsupported events', async () => {
  const legacy = await import('data:text/javascript;base64,' + Buffer.from(executable.replace(
    'const VERSION = globalThis.__pebrelPiVersion;', 'const VERSION = "0.80.3";',
  )).toString('base64'));
  const previous = process.env.PEBREL_HOOK_EXE;
  process.env.PEBREL_HOOK_EXE = 'test-hook';
  const firstEvent = events.length;
  try {
    const callbacks = new Map();
    legacy.default({ on: (kind, callback) => callbacks.set(kind, callback) });
    assert.equal(callbacks.has('agent_settled'), false);
    const ctx = context('legacy');
    await callbacks.get('agent_start')({}, ctx);
    await callbacks.get('agent_end')({ messages: [{ role: 'assistant', stopReason: 'stop' }] }, ctx);
    assert.deepEqual(events.slice(firstEvent).map(event => event.kind), ['prompt', 'done']);
    assert.equal(events.at(-1).stop_reason, 'stop');
  } finally {
    events.splice(firstEvent);
    if (previous === undefined) delete process.env.PEBREL_HOOK_EXE;
    else process.env.PEBREL_HOOK_EXE = previous;
  }
});

test('shutdown discards pending completion and a new turn cannot reuse an old result', async () => {
  const previous = process.env.PEBREL_HOOK_EXE;
  process.env.PEBREL_HOOK_EXE = 'test-hook';
  const firstEvent = events.length;
  try {
    const callbacks = new Map();
    bridge.default({ on: (kind, callback) => callbacks.set(kind, callback) });
    const ctx = context('shutdown');
    await callbacks.get('agent_start')({}, ctx);
    await callbacks.get('agent_end')({ messages: [{ role: 'assistant', stopReason: 'error' }] }, ctx);
    await callbacks.get('session_shutdown')({}, ctx);
    await callbacks.get('agent_settled')({}, ctx);
    assert.deepEqual(events.slice(firstEvent).map(event => event.kind), ['prompt', 'session-end']);
    await callbacks.get('session_start')({}, ctx);
    await callbacks.get('agent_start')({}, ctx);
    await callbacks.get('agent_settled')({}, ctx);
    assert.equal(events.at(-1).stop_reason, 'unknown');
  } finally {
    events.splice(firstEvent);
    if (previous === undefined) delete process.env.PEBREL_HOOK_EXE;
    else process.env.PEBREL_HOOK_EXE = previous;
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
    await callbacks.get('agent_end')({ messages: [{ role: 'assistant', stopReason: 'stop' }] }, context('second'));
    await callbacks.get('agent_settled')({}, context('second'));
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
