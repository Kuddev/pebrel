import http from 'node:http';
import https from 'node:https';
import { createHash, timingSafeEqual, randomUUID } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
import { WebSocketServer, WebSocket } from 'ws';
import { MAX_ENVELOPE, MAX_BUFFERED, devicePattern, tokenPattern } from './protocol.mjs';

const digest = value => createHash('sha256').update(value).digest();

/** User-owned rendezvous. No accounts, database, terminal replay or payload logging. */
export function createRelay(config, options = {}) {
  if (!Array.isArray(config.devices) || !config.devices.length || config.devices.length > 64) throw new Error('configure_1_to_64_devices');
  if (options.tls && (!options.tls.key || !options.tls.cert)) throw new Error('invalid_tls_config');
  const rooms = new Map();
  for (const device of config.devices) {
    if (!devicePattern.test(device.id) || rooms.has(device.id) ||
        !tokenPattern.test(device.desktopToken) || !tokenPattern.test(device.mobileToken) ||
        device.desktopToken === device.mobileToken) throw new Error('invalid_device_credentials');
    rooms.set(device.id, { keys: { desktop: digest(device.desktopToken), mobile: digest(device.mobileToken) }, peers: {}, link: null });
  }
  const requestHandler = (request, response) => {
    response.writeHead(request.url === '/healthz' ? 200 : 404, { 'content-type': 'text/plain' });
    response.end(request.url === '/healthz' ? 'ok\n' : 'not found\n');
  };
  const server = options.tls
    ? https.createServer({ key: options.tls.key, cert: options.tls.cert }, requestHandler)
    : http.createServer(requestHandler);
  server.maxConnections = 256;
  server.requestTimeout = 10_000;
  server.headersTimeout = 10_000;
  const sockets = new WebSocketServer({ noServer: true, maxPayload: MAX_ENVELOPE, perMessageDeflate: false });
  const send = (socket, value) => {
    if (!socket || socket.readyState !== WebSocket.OPEN) return false;
    if (socket.bufferedAmount > MAX_BUFFERED) { socket.terminate(); return false; }
    socket.send(JSON.stringify(value));
    return true;
  };
  server.on('upgrade', (request, socket, head) => {
    let accepted = false;
    try {
      const url = new URL(request.url, 'http://relay.invalid');
      const role = url.searchParams.get('role');
      const room = rooms.get(url.searchParams.get('device'));
      const header = request.headers.authorization ?? '';
      const token = header.startsWith('Bearer ') ? header.slice(7) : '';
      if (url.pathname !== '/v1/link' || !room || !['desktop', 'mobile'].includes(role) ||
          !tokenPattern.test(token) || !timingSafeEqual(digest(token), room.keys[role])) throw new Error('unauthorized');
      if (room.peers[role]) {
        socket.end('HTTP/1.1 409 Conflict\r\nConnection: close\r\n\r\n');
        return;
      }
      accepted = true;
      sockets.handleUpgrade(request, socket, head, ws => {
        room.peers[role] = ws;
        ws.alive = true;
        ws.on('pong', () => { ws.alive = true; });
        ws.on('error', () => { /* close handler owns cleanup; never log credentials */ });
        let count = 0;
        let windowStart = Date.now();
        ws.on('message', (bytes, binary) => {
          if (Date.now() - windowStart > 1000) { count = 0; windowStart = Date.now(); }
          if (++count > 100 || binary) { ws.close(1008, 'protocol_limit'); return; }
          try {
            const frame = JSON.parse(bytes.toString());
            if (!room.link || frame.type !== 'relay.data' || frame.link !== room.link || !frame.body || typeof frame.body !== 'object') throw new Error('invalid_link');
            const peer = room.peers[role === 'desktop' ? 'mobile' : 'desktop'];
            // No store-and-forward: a prompt can never cross into the next link.
            if (!peer || peer.readyState !== WebSocket.OPEN) throw new Error('peer_offline');
            if (peer.bufferedAmount + bytes.length > MAX_BUFFERED) { peer.terminate(); ws.close(1013, 'congested'); return; }
            peer.send(bytes, { binary: false });
          } catch { ws.close(1008, 'invalid_frame'); }
        });
        ws.on('close', () => {
          if (room.peers[role] !== ws) return;
          delete room.peers[role];
          room.link = null;
          send(room.peers[role === 'desktop' ? 'mobile' : 'desktop'], { type: 'relay.peer_left' });
        });
        if (room.peers.desktop && room.peers.mobile) {
          room.link = randomUUID();
          // Desktop gets the link first; its ready response is delivered after
          // both paired control frames have been queued by this event turn.
          send(room.peers.desktop, { type: 'relay.paired', link: room.link });
          send(room.peers.mobile, { type: 'relay.paired', link: room.link });
        } else send(ws, { type: 'relay.waiting' });
      });
    } catch {
      if (!accepted) socket.end('HTTP/1.1 401 Unauthorized\r\nConnection: close\r\n\r\n');
      else socket.destroy();
    }
  });
  const heartbeat = setInterval(() => {
    for (const socket of sockets.clients) {
      if (!socket.alive) { socket.terminate(); continue; }
      socket.alive = false;
      socket.ping();
    }
  }, 30_000);
  heartbeat.unref();
  return { server, close: async () => {
    clearInterval(heartbeat);
    for (const socket of sockets.clients) socket.terminate();
    await new Promise(resolve => sockets.close(resolve));
    await new Promise(resolve => server.close(resolve));
  } };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const config = JSON.parse(readFileSync(process.env.PEBREL_RELAY_CONFIG ?? './relay.config.json', 'utf8'));
  const relay = createRelay(config);
  relay.server.listen(Number(process.env.PORT ?? 8787), process.env.BIND ?? '127.0.0.1', () => console.log('Pebrel relay listening'));
  for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, () => { relay.close().then(() => process.exit(0)); });
}
