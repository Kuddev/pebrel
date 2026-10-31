import test from 'node:test';
import assert from 'node:assert/strict';
import { connectSession } from './session.mjs';

const flush = () => new Promise(resolve => setImmediate(resolve));
function fixture({ initialBusy = false } = {}) {
  let key;
  let response;
  let requests = 0;
  let status = { type: initialBusy ? 'busy' : 'idle' };
  let disposed = 0;
  const events = new Map();
  const native = { focused: true, current: { input: '', parts: [] },
    set(value) { this.current = value; }, submit() { assert.fail('must never submit'); } };
  const api = {
    route: { current: { name: 'session', params: { sessionID: 's1' } } },
    ui: { dialog: { open: false } },
    state: { session: {
      get: () => ({}), status: () => status,
      messages: () => [{ id: 'u1', role: 'user' },
        { id: 'a1', role: 'assistant', finish: 'stop', time: { completed: 1 } }],
    }, part: id => [{ type: 'text', text: id === 'u1' ? '再检查喵' : '检查完成。' }] },
    keymap: {
      createKeyMatcher: name => event => name === event.name,
      intercept: (_kind, fn) => { key = fn; return () => { disposed++; }; },
    },
    event: { on: (name, fn) => { events.set(name, fn); return () => { disposed++; }; } },
  };
  const adapter = connectSession({ api, session: 's1', prompt: () => native, editable: () => true,
    present: () => {}, transport: {
      config: async () => ({ enabled: true, loading_text: 'sol 正在预测' }),
      predict: body => { requests++; assert.equal(body.agent, 'opencode'); return new Promise(resolve => { response = resolve; }); },
    } });
  adapter.changed();
  const emit = type => {
    status = { type };
    events.get('session.status')({ properties: { sessionID: 's1', status } });
  };
  return { adapter, native, api, emit, key: name => {
    let consumed = false;
    key({ event: { name }, consume() { consumed = true; } });
    return consumed;
  }, resolve: suggestion => response({ suggestion }), requests: () => requests, disposed: () => disposed };
}

test('only a completed main turn predicts; native right-arrow accepts without sending', async () => {
  const f = fixture();
  f.emit('idle');
  await flush();
  assert.equal(f.requests(), 0);
  f.emit('busy');
  f.emit('idle');
  await flush();
  assert.equal(f.adapter.read().kind, 'loading');
  f.resolve('继续检查喵');
  await flush();
  assert.equal(f.key('right'), true);
  assert.equal(f.native.current.input, '继续检查喵');
  assert.equal(f.adapter.read().kind, 'hidden');
  f.adapter.dispose();
  assert.equal(f.disposed(), 3);
});

test('loading is not accepted, and any input invalidates the delayed candidate', async () => {
  const f = fixture();
  f.emit('busy'); f.emit('idle'); await flush();
  assert.equal(f.key('right'), false);
  assert.equal(f.adapter.read().kind, 'loading');
  f.native.current.input = '手动输入'; f.adapter.changed();
  f.resolve('旧建议'); await flush();
  assert.equal(f.key('right'), false);
  assert.equal(f.native.current.input, '手动输入');
  f.adapter.dispose();
});

test('a session prompt mounted during its first running turn predicts after settlement', async () => {
  const f = fixture({ initialBusy: true });
  f.emit('idle');
  await flush();
  assert.equal(f.requests(), 1);
  f.resolve('检查下一步');
  await flush();
  assert.equal(f.adapter.read().kind, 'ready');
  f.adapter.dispose();
});

test('attachment-only drafts and dialogs are not empty editable prompts', async () => {
  for (const block of [f => { f.native.current.parts.push({ type: 'file' }); },
    f => { f.api.ui.dialog.open = true; }, f => { f.api.route.current.params.sessionID = 's2'; },
    f => { f.api.state.session.get = () => ({ parentID: 'main' }); }]) {
    const f = fixture(); block(f); f.adapter.changed();
    f.emit('busy'); f.emit('idle'); await flush();
    assert.equal(f.requests(), 0);
    f.adapter.dispose();
  }
});

test('failed turns and interacting during a running turn never predict', async () => {
  for (const block of [f => { f.api.state.session.messages = () => [
    { role: 'assistant', error: {}, finish: 'stop', time: { completed: 1 } }]; },
    f => { f.key('!'); }]) {
    const f = fixture(); f.emit('busy'); block(f); f.emit('idle'); await flush();
    assert.equal(f.requests(), 0);
    f.adapter.dispose();
  }
});
