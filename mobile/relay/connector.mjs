import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
import { WebSocket } from 'ws';
import { MAX_ENVELOPE, MAX_BUFFERED, devicePattern, tokenPattern, ready } from './protocol.mjs';
import { RuntimeLink, discoverEndpoint } from './runtime-link.mjs';
import { pinnedTlsOptions, validateTlsPin } from './tls.mjs';

/** Outbound-only desktop connector; Windows/Linux/macOS need no inbound port. */
export function connectDesktop(config, options = {}) {
  const url = new URL(config.url);
  const loopback = ['127.0.0.1', 'localhost', '::1', '[::1]'].includes(url.hostname);
  if (url.protocol !== 'wss:' && !(options.allowLoopback && loopback && url.protocol === 'ws:')) throw new Error('wss_required');
  if (url.username || url.password || url.search || url.hash || !devicePattern.test(config.device) || !tokenPattern.test(config.token)) throw new Error('invalid_connection');
  const tlsPin = options.tlsPin ?? config.tlsPin;
  if (tlsPin) validateTlsPin(tlsPin);
  url.pathname = '/v1/link'; url.searchParams.set('device', config.device); url.searchParams.set('role', 'desktop');
  let stopped = false;
  let socket;
  let runtime;
  let link;
  let reconnect;
  let attempts = 0;
  let runtimeUnavailable = false;
  const status = options.onStatus ?? (() => {});
  const release = () => { runtime?.close(); runtime = null; link = null; };
  const send = body => {
    if (!link || !socket || socket.readyState !== WebSocket.OPEN || socket.bufferedAmount > MAX_BUFFERED) {
      socket?.terminate(); return;
    }
    const bytes = JSON.stringify({ type: 'relay.data', link, body });
    if (Buffer.byteLength(bytes) > MAX_ENVELOPE) { socket.terminate(); return; }
    socket.send(bytes);
  };
  const reportRuntimeUnavailable = () => {
    if (runtimeUnavailable) return;
    runtimeUnavailable = true;
    status('runtime_unavailable');
    // Keep this diagnostic deliberately bounded. It is consumed by Android and
    // must never carry a filesystem error, token, or other local detail.
    send({ type: 'mobile.disconnected', code: 'runtime_unavailable' });
    socket.close(1008, 'runtime_unavailable');
  };
  const start = () => {
    if (stopped) return;
    runtimeUnavailable = false;
    status('connecting');
    socket = new WebSocket(url, { headers: { Authorization: `Bearer ${config.token}` },
      maxPayload: MAX_ENVELOPE, perMessageDeflate: false, handshakeTimeout: 15_000,
      ...pinnedTlsOptions({ pin: tlsPin, certificate: options.tlsCert ?? config.tlsCert }) });
    let heartbeat;
    let lastReceived = Date.now();
    socket.on('open', () => {
      status('waiting_for_phone');
      heartbeat = setInterval(() => {
        if (Date.now() - lastReceived > 90_000) socket.terminate();
      }, 30_000);
      heartbeat.unref();
    });
    socket.on('ping', () => { lastReceived = Date.now(); });
    socket.on('message', bytes => {
      lastReceived = Date.now();
      try {
        const frame = JSON.parse(bytes.toString());
        if (frame.type === 'relay.paired') {
          release(); link = frame.link; attempts = 0;
          let endpoint;
          try {
            endpoint = options.endpoint ?? discoverEndpoint(config.runtimeFile);
          } catch {
            reportRuntimeUnavailable();
            return;
          }
          runtime = new RuntimeLink(endpoint, options.allowInput === true, send, () => {
            reportRuntimeUnavailable();
            release();
          });
          send(ready(options.allowInput === true)); status('paired');
        } else if (frame.type === 'relay.peer_left') { release(); status('waiting_for_phone'); }
        else if (frame.type === 'relay.data' && frame.link === link) runtime?.request(frame.body);
        else if (frame.type !== 'relay.waiting') throw new Error('invalid_frame');
      } catch {
        if (!runtimeUnavailable) status('connection_failed');
        socket.close(1008, 'protocol_error');
      }
    });
    socket.on('unexpected-response', (_request, response) => {
      // Rejected/revoked credentials require user action, never an endless retry storm.
      if ([401, 403, 409].includes(response.statusCode)) {
        status(response.statusCode === 409 ? 'device_already_connected' : 'authentication_failed');
        stopped = true;
      } else status('server_unavailable');
      response.resume(); socket.terminate();
    });
    socket.on('error', () => { if (!runtimeUnavailable) status('connection_failed'); });
    socket.on('close', () => {
      clearInterval(heartbeat); release();
      if (!stopped) {
        // Leave the actionable Runtime diagnosis visible until the next retry
        // starts. Generic network failures retain the existing reconnect state.
        if (!runtimeUnavailable) status('reconnecting');
        const ceiling = Math.min(30_000, 1000 * 2 ** Math.min(attempts++, 5));
        reconnect = setTimeout(start, ceiling / 2 + Math.random() * ceiling / 2);
      }
    });
  };
  start();
  return { close() { stopped = true; clearTimeout(reconnect); release(); socket?.terminate(); } };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const index = process.argv.indexOf('--config');
  if (index < 0 || !process.argv[index + 1]) throw new Error('Usage: node connector.mjs --config computer.json [--allow-input]');
  const config = JSON.parse(readFileSync(process.argv[index + 1], 'utf8'));
  const connector = connectDesktop(config, { allowInput: process.argv.includes('--allow-input'), onStatus: value => console.log(value) });
  for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, () => { connector.close(); });
}
