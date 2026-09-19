import { readFileSync } from 'node:fs';

export const policy = JSON.parse(readFileSync(new URL('../protocol/bridge-policy.json', import.meta.url), 'utf8'));
export const MAX_ENVELOPE = policy.maxFrameBytes + 1024;
export const MAX_BUFFERED = MAX_ENVELOPE * 2;
export const devicePattern = /^[a-zA-Z0-9_-]{1,64}$/;
export const tokenPattern = /^[a-zA-Z0-9_-]{43}$/;

export function validateRequest(request, allowInput) {
  if (!request || typeof request !== 'object' || Array.isArray(request) ||
      Object.keys(request).some(key => !['id', 'method', 'params'].includes(key))) throw new Error('invalid_request');
  if (typeof request.id !== 'string' || !request.id.length || Buffer.byteLength(request.id) > 80 || /\p{Cc}/u.test(request.id)) throw new Error('invalid_id');
  const params = request.params ?? {};
  if (typeof params !== 'object' || params === null || Array.isArray(params)) throw new Error('invalid_params');
  if (!policy.read.includes(request.method)) {
    if (!policy.input.includes(request.method)) throw new Error('method_not_found');
    if (!allowInput) throw new Error('input_not_authorized');
  }
  if (request.method.startsWith('pane.')) {
    for (const key of ['window_id', 'pane_id']) if (!Number.isSafeInteger(params[key]) || params[key] <= 0) throw new Error('invalid_target');
  }
  if (request.method === 'tab.new' || request.method === 'tab.close') {
    if (!Number.isSafeInteger(params.window_id) || params.window_id <= 0) throw new Error('invalid_target');
  }
  if (request.method === 'tab.close') {
    if (!Number.isSafeInteger(params.tab_index) || params.tab_index < 0 ||
        !Number.isSafeInteger(params.expected_pane_id) || params.expected_pane_id <= 0) throw new Error('invalid_target');
  }
  if (Buffer.byteLength(JSON.stringify(request)) > policy.maxRequestBytes) throw new Error('request_too_large');
  return { id: request.id, method: request.method, params };
}

export function failure(id, code) { return { id, ok: false, error: { code, message: code } }; }
export function ready(allowInput) {
  return { type: 'mobile.ready', protocol: 'pebrel.mobile.relay', version: 1,
    capabilities: { snapshot: true, read_tail: true, state_subscription: true, input: allowInput,
      exclusive_input: false, replay_notifications: false, terminal_grid_stream: false,
      tab_create: allowInput, tab_close: allowInput },
    max_request_bytes: policy.maxRequestBytes, max_frame_bytes: policy.maxFrameBytes };
}
