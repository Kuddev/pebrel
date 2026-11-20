import net from 'node:net';
import { readFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { policy, validateRequest, failure } from './protocol.mjs';

export function discoverEndpoint(runtimeFile) {
  let value = process.env.PEBREL_RUNTIME_ENDPOINT;
  if (!value) {
    let directory = process.env.PEBREL_CONFIG_DIR || process.env.NEBULA_CONFIG_DIR;
    if (!directory) directory = process.platform === 'win32'
      ? path.join(process.env.APPDATA || path.join(os.homedir(), 'AppData', 'Roaming'), 'Pebrel')
      : process.platform === 'darwin' ? path.join(os.homedir(), 'Library', 'Application Support', 'Pebrel')
      : path.join(process.env.XDG_CONFIG_HOME || path.join(os.homedir(), '.config'), 'pebrel');
    value = readFileSync(runtimeFile || path.join(directory, 'runtime.port'), 'utf8');
  }
  const [port, token] = value.trim().split(/\s+/);
  if (!/^\d+$/.test(port) || +port <= 0 || +port > 65535 || !/^[a-fA-F0-9]{32}$/.test(token)) throw new Error('invalid_runtime_endpoint');
  return { port: +port, token };
}

/** One paired peer owns these loopback requests. The endpoint never changes mid-link. */
export class RuntimeLink {
  constructor(endpoint, allowInput, emit, disconnect) {
    this.endpoint = endpoint;
    this.allowInput = allowInput;
    this.emit = emit;
    this.disconnect = disconnect;
    this.active = new Map();
    this.seen = new Set();
    this.inputIds = new Set();
    this.subscribed = false;
    this.closed = false;
  }
  request(raw) {
    if (this.closed) return;
    let request;
    try { request = validateRequest(raw, this.allowInput); }
    catch (error) { this.emit(failure(typeof raw?.id === 'string' ? raw.id.slice(0, 80) : 'invalid', error.message)); return; }
    if (this.seen.has(request.id) || this.inputIds.has(request.id)) { this.emit(failure(request.id, 'duplicate_request')); return; }
    if (this.active.size >= policy.maxPending) { this.emit(failure(request.id, 'too_many_requests')); return; }
    const subscription = request.method === 'events.subscribe';
    if (subscription && this.subscribed) { this.emit(failure(request.id, 'already_subscribed')); return; }
    if (policy.input.includes(request.method)) {
      // Polling must never evict an accepted input ID. Bound the lifetime budget
      // instead: only an explicit new pairing permits more inputs after it fills.
      if (this.inputIds.size >= 4096) { this.emit(failure(request.id, 'input_limit_reconnect')); return; }
      this.inputIds.add(request.id);
    }
    this.seen.add(request.id);
    if (this.seen.size > 256) this.seen.delete(this.seen.values().next().value);
    if (subscription) this.subscribed = true;
    const socket = net.createConnection({ host: '127.0.0.1', port: this.endpoint.port });
    this.active.set(request.id, socket);
    socket.setNoDelay(true);
    socket.setTimeout(35_000);
    let fragments = [];
    let length = 0;
    let responded = false;
    let ended = false;
    const lost = () => {
      if (ended || this.closed) return;
      ended = true;
      if (!responded) this.emit(failure(request.id, 'delivery_unknown'));
      // Never rediscover a newly started desktop and silently retarget input.
      this.disconnect();
      this.close();
    };
    socket.on('connect', () => {
      const local = { protocol: 'nebula.runtime', version: 1, ...request, token: this.endpoint.token };
      socket.end(JSON.stringify(local) + '\n');
    });
    socket.on('data', bytes => {
      let start = 0;
      while (start < bytes.length && !this.closed) {
        const newline = bytes.indexOf(10, start);
        const end = newline < 0 ? bytes.length : newline + 1;
        const fragment = bytes.subarray(start, end);
        length += fragment.length;
        if (length > policy.maxFrameBytes) { lost(); socket.destroy(); return; }
        fragments.push(fragment);
        start = end;
        if (newline < 0) break;
        try {
          const frame = JSON.parse(Buffer.concat(fragments, length).toString('utf8'));
          fragments = []; length = 0;
          if (!responded) {
            if (frame.id !== request.id || typeof frame.ok !== 'boolean') throw new Error('invalid_response');
            responded = true;
            if (subscription && frame.ok) socket.setTimeout(0);
          } else if (!subscription || frame.event !== 'runtime.snapshot') throw new Error('invalid_event');
          this.emit(frame);
        } catch { lost(); socket.destroy(); return; }
      }
    });
    socket.on('timeout', () => { lost(); socket.destroy(); });
    socket.on('error', lost);
    socket.on('close', () => {
      this.active.delete(request.id);
      if (length || !responded || subscription) lost();
    });
  }
  close() {
    if (this.closed) return;
    this.closed = true;
    for (const socket of this.active.values()) socket.destroy();
    this.active.clear(); this.seen.clear(); this.inputIds.clear();
  }
}
