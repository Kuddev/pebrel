import test from 'node:test';
import assert from 'node:assert/strict';
import net from 'node:net';
import http from 'node:http';
import { once } from 'node:events';
import { WebSocket } from 'ws';
import { createRelay } from '../server.mjs';
import { connectDesktop } from '../connector.mjs';
import { validateRequest } from '../protocol.mjs';

const device = { id: 'test-device', desktopToken: 'd'.repeat(43), mobileToken: 'm'.repeat(43) };
const snapshot = { process_id: 42, windows: [{ id: 1, tabs: [{ label: 'Test', panes: [
  { id: 2, title: 'shell', cwd: '/work', running_program: null, task_state: 'idle', state_change_seq: 1 },
] }] }] };

function inbox(socket) {
  const queue = [];
  const listeners = [];
  socket.on('message', bytes => {
    const value = JSON.parse(bytes.toString());
    const waiter = listeners.shift();
    if (waiter) waiter(value); else queue.push(value);
  });
  return async () => {
    if (queue.length) return queue.shift();
    return new Promise(resolve => listeners.push(resolve));
  };
}
async function relayFixture(t) {
  const relay = createRelay({ devices: [device] });
  relay.server.listen(0, '127.0.0.1');
  await once(relay.server, 'listening');
  t.after(() => relay.close());
  const url = `ws://127.0.0.1:${relay.server.address().port}`;
  const open = role => {
    const ws = new WebSocket(`${url}/v1/link?device=${device.id}&role=${role}`, {
      headers: { Authorization: `Bearer ${device[`${role}Token`]}` },
    });
    const next = inbox(ws);
    return { ws, next };
  };
  return { url, open };
}
async function runtimeFixture(t) {
  const sockets = new Set();
  const requests = [];
  const server = net.createServer({ allowHalfOpen: true }, socket => {
    sockets.add(socket); socket.on('close', () => sockets.delete(socket)); socket.on('error', () => {});
    let buffer = '';
    socket.on('data', bytes => {
      buffer += bytes;
      if (!buffer.endsWith('\n')) return;
      const request = JSON.parse(buffer);
      requests.push(request);
      assert.equal(request.token, 'a'.repeat(32));
      const result = request.method === 'pane.read' ? { text: 'terminal output' } : {};
      socket.write(JSON.stringify({ id: request.id, ok: true, result }) + '\n');
      if (request.method === 'events.subscribe') socket.write(JSON.stringify({ event: 'runtime.snapshot', data: snapshot }) + '\n');
      else socket.end();
    });
  });
  server.listen(0, '127.0.0.1'); await once(server, 'listening');
  t.after(async () => { for (const socket of sockets) socket.destroy(); await new Promise(resolve => server.close(resolve)); });
  return { endpoint: { port: server.address().port, token: 'a'.repeat(32) }, requests };
}

test('role credentials cannot be interchanged and duplicate peers cannot replace owners', { timeout: 8000 }, async t => {
  const { url, open } = await relayFixture(t);
  const rejected = new WebSocket(`${url}/v1/link?device=${device.id}&role=desktop`, { headers: { Authorization: `Bearer ${device.mobileToken}` } });
  rejected.on('error', () => {});
  const response = await new Promise(resolve => rejected.on('unexpected-response', (_request, response) => { resolve(response.statusCode); response.resume(); rejected.terminate(); }));
  assert.equal(response, 401);
  const desktop = open('desktop'); assert.equal((await desktop.next()).type, 'relay.waiting');
  const duplicate = open('desktop'); duplicate.ws.on('error', () => {});
  const status = await new Promise(resolve => duplicate.ws.on('unexpected-response', (_request, response) => { resolve(response.statusCode); response.resume(); duplicate.ws.terminate(); }));
  assert.equal(status, 409);
  assert.equal(desktop.ws.readyState, WebSocket.OPEN);
});

test('phone reaches the real loopback protocol through relay and reconnect does not replay input', { timeout: 12000 }, async t => {
  const { url, open } = await relayFixture(t);
  const { endpoint, requests } = await runtimeFixture(t);
  let waitForPhone;
  const hostOnline = new Promise(resolve => { waitForPhone = resolve; });
  const connector = connectDesktop({ url, device: device.id, token: device.desktopToken }, {
    allowLoopback: true, allowInput: true, endpoint,
    onStatus(value) { if (value === 'waiting_for_phone') waitForPhone(); },
  });
  t.after(() => connector.close());
  await hostOnline;
  const mobile = open('mobile');
  const pair = await mobile.next(); assert.equal(pair.type, 'relay.paired');
  const hello = await mobile.next(); assert.equal(hello.body.protocol, 'pebrel.mobile.relay');
  assert.equal(hello.body.capabilities.input, true);
  const send = body => mobile.ws.send(JSON.stringify({ type: 'relay.data', link: pair.link, body }));
  send({ id: 'subscribe', method: 'events.subscribe', params: {} });
  assert.equal((await mobile.next()).body.ok, true);
  assert.deepEqual((await mobile.next()).body.data, snapshot);
  send({ id: 'read', method: 'pane.read', params: { window_id: 1, pane_id: 2, lines: 120 } });
  assert.equal((await mobile.next()).body.result.text, 'terminal output');
  send({ id: 'write', method: 'pane.prompt', params: { window_id: 1, pane_id: 2, text: 'pwd', submit: true } });
  assert.equal((await mobile.next()).body.ok, true);
  send({ id: 'write', method: 'pane.prompt', params: { window_id: 1, pane_id: 2, text: 'pwd', submit: true } });
  assert.equal((await mobile.next()).body.error.code, 'duplicate_request');
  for (let index = 0; index < 257; index++) {
    // Cross the dedup history size while respecting the real broker's 100/s
    // limit; this is a long-lived polling scenario, not a flood exemption.
    if (index % 64 === 0) await new Promise(resolve => setTimeout(resolve, 1050));
    send({ id: `poll-${index}`, method: 'runtime.describe', params: {} });
    assert.equal((await mobile.next()).body.ok, true);
  }
  send({ id: 'write', method: 'pane.prompt', params: { window_id: 1, pane_id: 2, text: 'pwd', submit: true } });
  assert.equal((await mobile.next()).body.error.code, 'duplicate_request');
  const waiting = new Promise(resolve => { waitForPhone = resolve; });
  mobile.ws.close(); await waiting;
  const second = open('mobile');
  const newPair = await second.next(); await second.next();
  assert.notEqual(newPair.link, pair.link);
  const closed = once(second.ws, 'close');
  second.ws.send(JSON.stringify({ type: 'relay.data', link: pair.link, body: { id: 'old', method: 'pane.prompt', params: { window_id: 1, pane_id: 2, text: 'pwd' } } }));
  await closed;
  assert.equal(requests.filter(request => request.method === 'pane.prompt').length, 1);
});

test('temporary proxy failures reconnect while rejected credentials stop retries', { timeout: 8000 }, async t => {
  let attempts = 0;
  const server = http.createServer();
  server.on('upgrade', (_request, socket) => {
    attempts++;
    const response = attempts === 1 ? '503 Service Unavailable' : '401 Unauthorized';
    socket.end(`HTTP/1.1 ${response}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n`);
  });
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  t.after(() => new Promise(resolve => server.close(resolve)));
  const statuses = [];
  let rejected;
  const rejection = new Promise(resolve => { rejected = resolve; });
  const connector = connectDesktop({ url: `ws://127.0.0.1:${server.address().port}`, device: device.id, token: device.desktopToken }, {
    allowLoopback: true,
    onStatus(value) { statuses.push(value); if (value === 'authentication_failed') rejected(); },
  });
  t.after(() => connector.close());
  await rejection;
  // Drain the close event so an erroneous reconnect scheduling is observable.
  await new Promise(resolve => setTimeout(resolve, 1200));
  assert.equal(attempts, 2);
  assert.ok(statuses.includes('server_unavailable'));
  assert.equal(statuses.filter(value => value === 'reconnecting').length, 1);
});

test('read-only channels reject input, caller tokens and incomplete pane identities', () => {
  const params = { window_id: 1, pane_id: 2 };
  assert.throws(() => validateRequest({ id: 'x', method: 'pane.prompt', params }, false), /input_not_authorized/);
  assert.throws(() => validateRequest({ id: 'x', method: 'runtime.snapshot', token: 'caller' }, false), /invalid_request/);
  assert.throws(() => validateRequest({ id: 'x', method: 'pane.read', params: { pane_id: 2 } }, false), /invalid_target/);
  assert.throws(() => validateRequest({ id: 'x', method: 'window.close', params }, true), /method_not_found/);
  assert.equal(validateRequest({ id: 'x', method: 'pane.read', params }, false).method, 'pane.read');
});
