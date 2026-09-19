import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, readdirSync, rmSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const script = fileURLToPath(new URL('../init.mjs', import.meta.url));

test('initialization creates matching role credentials and retains existing computers and deployment settings', t => {
  const directory = mkdtempSync(path.join(tmpdir(), 'pebrel-relay-init-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const initialize = name => {
    const result = spawnSync(process.execPath, [script, '--url', 'wss://relay.example.com', '--name', name], {
      cwd: directory, encoding: 'utf8',
    });
    assert.equal(result.status, 0, result.stderr);
    return result.stdout;
  };
  const firstOutput = initialize('Windows PC');
  const privateDirectory = path.join(directory, 'private');
  const configuration = () => JSON.parse(readFileSync(path.join(privateDirectory, 'relay.config.json'), 'utf8'));
  const first = configuration().devices[0];
  const deployment = readFileSync(path.join(directory, '.env'), 'utf8');
  assert.match(deployment, /^PEBREL_RELAY_DOMAIN=relay\.example\.com\n/);
  const secondOutput = initialize('MacBook');
  const devices = configuration().devices;
  assert.equal(devices.length, 2);
  assert.deepEqual(devices[0], first);
  assert.equal(readFileSync(path.join(directory, '.env'), 'utf8'), deployment);
  const secrets = new Set();
  for (const device of devices) {
    const computer = JSON.parse(readFileSync(path.join(privateDirectory, `computer-${device.id}.json`), 'utf8'));
    const phone = JSON.parse(readFileSync(path.join(privateDirectory, `phone-${device.id}.txt`), 'utf8'));
    const phoneQr = readFileSync(path.join(privateDirectory, `phone-${device.id}.svg`), 'utf8');
    assert.equal(computer.device, device.id);
    assert.equal(phone.device, device.id);
    assert.equal(computer.token, device.desktopToken);
    assert.equal(phone.token, device.mobileToken);
    assert.match(phoneQr, /^<svg[^>]+>/);
    assert.ok(!phoneQr.includes(phone.token));
    for (const token of [computer.token, phone.token]) {
      assert.match(token, /^[a-zA-Z0-9_-]{43}$/);
      assert.ok(!secrets.has(token));
      assert.ok(!(firstOutput + secondOutput).includes(token));
      secrets.add(token);
    }
  }
  if (process.platform !== 'win32') {
    for (const file of readdirSync(privateDirectory)) {
      assert.equal(statSync(path.join(privateDirectory, file)).mode & 0o077, 0);
    }
    assert.equal(statSync(path.join(directory, '.env')).mode & 0o077, 0);
  }
});

test('invalid deployment URLs create no credentials or Compose environment', t => {
  const directory = mkdtempSync(path.join(tmpdir(), 'pebrel-relay-invalid-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  for (const url of ['ws://relay.example.com', 'wss://relay.example.com:8443', 'wss://relay.example.com/path', 'wss://user:secret@relay.example.com']) {
    const result = spawnSync(process.execPath, [script, '--url', url], { cwd: directory, encoding: 'utf8' });
    assert.notEqual(result.status, 0);
    assert.deepEqual(readdirSync(directory), []);
  }
});
