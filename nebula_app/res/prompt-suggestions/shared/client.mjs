// Only transport and ephemeral UI lifecycle live here. Prompt/model policy belongs
// to Pebrel's host; native adapters own input identity and completed-turn events.
export function conversation(messages) {
  const result = [];
  let remaining = 12000;
  for (const message of messages.slice(-24).reverse()) {
    if (!['user', 'assistant'].includes(message.role) || typeof message.text !== 'string') continue;
    const chars = [];
    const limit = Math.min(3000, remaining);
    for (const char of message.text.trim()) {
      if (chars.length === limit) break;
      chars.push(char);
    }
    if (!chars.length) continue;
    result.push({ role: message.role, text: chars.join('') });
    remaining -= chars.length;
    if (!remaining) break;
  }
  return result.reverse();
}

export function validSuggestion(text) {
  return typeof text === 'string' && text.trim() === text && text.length > 0
    && Array.from(text).length <= 240 && !/[\x00-\x1f\x7f-\x9f\u2028\u2029]/u.test(text);
}

export function createTransport(env = process.env, fetcher = fetch) {
  let endpoint;
  try { endpoint = new URL(env.PEBREL_PREDICTION_ENDPOINT); } catch { return undefined; }
  const token = env.PEBREL_PREDICTION_TOKEN;
  if (endpoint.protocol !== 'http:' || !['127.0.0.1', '[::1]'].includes(endpoint.hostname)
      || endpoint.username || endpoint.password || endpoint.pathname !== '/'
      || endpoint.search || endpoint.hash || !token || /[\r\n]/u.test(token)) return undefined;
  async function request(path, signal, body) {
    const encoded = body === undefined ? undefined : JSON.stringify(body);
    if (encoded && new TextEncoder().encode(encoded).length > 128 * 1024) throw new Error('request too large');
    const response = await fetcher(new URL(path, endpoint), {
      method: body === undefined ? 'GET' : 'POST',
      headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' },
      body: encoded, signal, redirect: 'error',
    });
    if (!response.ok || !response.body) throw new Error('prediction unavailable');
    const reader = response.body.getReader();
    const chunks = [];
    let bytes = 0;
    try {
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        bytes += value.byteLength;
        if (bytes > 8192) throw new Error('response too large');
        chunks.push(value);
      }
    } finally {
      await reader.cancel();
      reader.releaseLock();
    }
    const joined = new Uint8Array(bytes);
    let offset = 0;
    for (const chunk of chunks) { joined.set(chunk, offset); offset += chunk.byteLength; }
    return JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(joined));
  }
  return {
    config: signal => request('/v1/prompt-suggestions/config', signal),
    predict: (body, signal) => request('/v1/prompt-suggestions', signal, body),
  };
}

export function createPrediction({ agent, transport, snapshot, present, insert,
  clock = globalThis, timeoutMs = 15000 }) {
  let generation = 0;
  let abort;
  let timer;
  let timeout;
  let state = { kind: 'hidden', text: '' };
  let disposed = false;
  const seen = new Set();
  const eligible = current => current && current.empty && current.editable && current.idle;
  const same = (a, b) => eligible(a) && a.session === b.session && a.revision === b.revision;
  const publish = value => { state = value; present(value); };
  function cancel() {
    ++generation;
    abort?.abort();
    abort = undefined;
    clock.clearInterval(timer);
    clock.clearTimeout(timeout);
    timer = timeout = undefined;
    if (state.kind !== 'hidden') publish({ kind: 'hidden', text: '' });
  }
  function validate() {
    if (state.kind !== 'hidden' && !same(snapshot(), state.owner)) cancel();
  }
  return {
    read: () => state,
    cancel,
    validate,
    async completed(turn, messages) {
      const owner = { ...snapshot() };
      if (disposed || !transport || !eligible(owner) || !turn) return;
      const key = `${owner.session}\0${turn}`;
      if (seen.has(key)) return;
      seen.add(key);
      if (seen.size > 32) seen.delete(seen.values().next().value);
      cancel();
      const run = generation;
      const controller = new AbortController();
      abort = controller;
      const current = () => !disposed && run === generation && same(snapshot(), owner);
      timeout = clock.setTimeout(() => { if (run === generation) cancel(); }, timeoutMs);
      try {
        const config = await transport.config(controller.signal);
        if (!current() || !config.enabled || typeof config.loading_text !== 'string') return;
        let dots = 1;
        publish({ kind: 'loading', text: `${config.loading_text}.`, owner });
        timer = clock.setInterval(() => {
          if (!current()) { cancel(); return; }
          dots = dots % 3 + 1;
          publish({ kind: 'loading', text: `${config.loading_text}${'.'.repeat(dots)}`, owner });
        }, 400);
        const reply = await transport.predict({ agent, session: owner.session, turn,
          revision: owner.revision, messages: conversation(messages) }, controller.signal);
        if (!current()) return;
        if (validSuggestion(reply.suggestion)) {
          publish({ kind: 'ready', text: reply.suggestion, owner });
        } else publish({ kind: 'hidden', text: '' });
      } catch {
        if (current()) publish({ kind: 'hidden', text: '' });
      } finally {
        if (run === generation) {
          clock.clearInterval(timer);
          clock.clearTimeout(timeout);
          timer = timeout = abort = undefined;
          // A focus/session change during a response must also clear loading.
          if (!same(snapshot(), owner)) cancel();
        }
      }
    },
    accept() {
      validate();
      if (disposed || state.kind !== 'ready') return false;
      const text = state.text;
      cancel();
      insert(text);
      return true;
    },
    dispose() { disposed = true; cancel(); seen.clear(); },
  };
}
