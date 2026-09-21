// Execute the shipped bridge, including its bounded native-header reader.
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync, mkdirSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { stripTypeScriptTypes } from 'node:module';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { test, after } from 'node:test';
import { spawnSync } from 'node:child_process';

const root = new URL('../../', import.meta.url);
const bridgeSource = new URL('nebula_app/src/ai_hook/bridges.rs', root);
const bridgePath = readFileSync(bridgeSource, 'utf8').match(/PI_EXTENSION_TS: &str = include_str!\("([^"]+)"\)/)?.[1];
assert.ok(bridgePath, 'test must execute the production PI_EXTENSION_TS asset');
const embedded = readFileSync(new URL(bridgePath, bridgeSource), 'utf8');
const events = [];
globalThis.__pebrelBridgeSpawn = (_hook, args) => {
  events.push(JSON.parse(args[1]));
  return { on() {}, unref() {} };
};
globalThis.__pebrelPiVersion = '0.85.1';
globalThis.__pebrelLoadPiModule = async () => ({ VERSION: globalThis.__pebrelPiVersion });
const executable = stripTypeScriptTypes(embedded.replace(
  'import { spawn } from "node:child_process";',
  'const spawn = globalThis.__pebrelBridgeSpawn;',
).replace(
  'await import(name)',
  'await globalThis.__pebrelLoadPiModule(name)',
) + '\nexport { sessionFor };');
const bridge = await import('data:text/javascript;base64,' + Buffer.from(executable).toString('base64'));
mkdirSync(new URL('tmp/', root), { recursive: true });
const directory = mkdtempSync(fileURLToPath(new URL('tmp/pi-bridge-', root)));
after(() => rmSync(directory, { recursive: true }));

test('a missing helper fails silently without terminating the agent process', () => {
  const asset = join(directory, 'missing-helper-bridge.mjs');
  writeFileSync(asset, stripTypeScriptTypes(embedded));
  const child = spawnSync(process.execPath, ['--input-type=module', '-e', `
    const bridge = await import(${JSON.stringify(pathToFileURL(asset).href)});
    const callbacks = new Map();
    await bridge.default({ on: (event, callback) => callbacks.set(event, callback) });
    await callbacks.get('agent_start')({}, {});
    await new Promise(resolve => setTimeout(resolve, 20));
    console.log('agent-alive');
  `], {
    encoding: 'utf8', windowsHide: true,
    env: { ...process.env, PEBREL_HOOK_EXE: join(directory, 'missing-helper.exe') },
  });
  assert.equal(child.status, 0, child.stderr);
  assert.match(child.stdout, /agent-alive/);
  assert.doesNotMatch(child.stderr, /Unhandled|ENOENT/);
});
const context = (id, file) => ({
  sessionManager: { getSessionId: () => id, getSessionFile: () => file },
});

test('the unchanged runtime imports load with old, new, absent and malformed SDK versions', async () => {
  const cases = [
    ['@mariozechner/pi-coding-agent', '0.60.0', false],
    ['@earendil-works/pi-coding-agent', '0.80.3', false],
    ['@earendil-works/pi-coding-agent', '0.80.4', true],
    ['@earendil-works/pi-coding-agent', '0.85.1', true],
    ['@earendil-works/pi-coding-agent', 'invalid', false],
    [null, null, false],
  ];
  for (const [name, version, settled] of cases) {
    const sandbox = mkdtempSync(join(directory, 'loader-'));
    const asset = join(sandbox, 'bridge.mjs');
    // Only erase TypeScript; keep every production import and the factory intact.
    writeFileSync(asset, stripTypeScriptTypes(embedded));
    if (name) {
      const sdk = join(sandbox, 'node_modules', name);
      mkdirSync(sdk, { recursive: true });
      writeFileSync(join(sdk, 'package.json'), JSON.stringify({ type: 'module', exports: './index.js' }));
      writeFileSync(join(sdk, 'index.js'), `export const VERSION = ${JSON.stringify(version)};`);
    }
    const loaded = await import(pathToFileURL(asset));
    const callbacks = new Map();
    await loaded.default({ on: (name, callback) => callbacks.set(name, callback) });
    assert.equal(callbacks.has('agent_settled'), settled, `${name} ${version}`);
    for (const event of ['agent_start', 'agent_end', 'session_start', 'session_shutdown']) {
      assert.equal(typeof callbacks.get(event), 'function');
    }
  }
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
    await bridge.default({ on: (kind, callback) => callbacks.set(kind, callback) });
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
      await bridge.default({ on: (kind, callback) => callbacks.set(kind, callback) });
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
    await bridge.default({ on: (kind, callback) => callbacks.set(kind, callback) });
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
  const previousVersion = globalThis.__pebrelPiVersion;
  globalThis.__pebrelPiVersion = '0.80.3';
  const legacy = bridge;
  const previous = process.env.PEBREL_HOOK_EXE;
  process.env.PEBREL_HOOK_EXE = 'test-hook';
  const firstEvent = events.length;
  try {
    const callbacks = new Map();
    await legacy.default({ on: (kind, callback) => callbacks.set(kind, callback) });
    assert.equal(callbacks.has('agent_settled'), false);
    const ctx = context('legacy');
    await callbacks.get('agent_start')({}, ctx);
    await callbacks.get('agent_end')({ messages: [{ role: 'assistant', stopReason: 'stop' }] }, ctx);
    assert.deepEqual(events.slice(firstEvent).map(event => event.kind), ['prompt', 'done']);
    assert.equal(events.at(-1).stop_reason, 'stop');
  } finally {
    globalThis.__pebrelPiVersion = previousVersion;
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
    await bridge.default({ on: (kind, callback) => callbacks.set(kind, callback) });
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
    await bridge.default({ on: (kind, callback) => callbacks.set(kind, callback) });
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
