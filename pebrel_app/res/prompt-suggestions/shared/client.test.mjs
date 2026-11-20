import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { conversation, createPrediction, createTransport, textExcerpt } from './client.mjs';

const flush = () => new Promise(resolve => setImmediate(resolve));
function fixture() {
  let resolve;
  let calls = 0;
  let signal;
  const current = { session: 'session-a', revision: 0, empty: true, editable: true, idle: true };
  const shown = [];
  const inserted = [];
  const intervals = new Set();
  const timeouts = new Set();
  const clock = {
    setInterval: (fn, ms) => { assert.equal(ms, 400); intervals.add(fn); return fn; },
    clearInterval: fn => intervals.delete(fn),
    setTimeout: fn => { timeouts.add(fn); return fn; },
    clearTimeout: fn => timeouts.delete(fn),
  };
  const prediction = createPrediction({ agent: 'pi', snapshot: () => current, clock,
    transport: {
      config: async () => ({ enabled: true, loading_text: 'sol 正在预测' }),
      predict: (_body, nextSignal) => { calls++; signal = nextSignal; return new Promise(done => { resolve = done; }); },
    },
    present: value => shown.push(value), insert: text => inserted.push(text),
  });
  return { prediction, current, shown, inserted, intervals, timeouts,
    resolve: value => resolve(value), calls: () => calls, signal: () => signal };
}

test('loading cycles at 400 ms; only the complete candidate can be accepted', async () => {
  const f = fixture();
  const work = f.prediction.completed('turn-1', [{ role: 'user', text: '喵，检查测试' }]);
  await flush();
  assert.equal(f.prediction.read().text, 'sol 正在预测.');
  assert.equal(f.prediction.accept(), false);
  for (let i = 0; i < 3; i++) for (const tick of f.intervals) tick();
  assert.deepEqual(f.shown.slice(-3).map(x => x.text), ['sol 正在预测..', 'sol 正在预测...', 'sol 正在预测.']);
  f.resolve({ suggestion: '再跑一遍测试喵' });
  await work;
  assert.equal(f.intervals.size, 0);
  assert.equal(f.timeouts.size, 0);
  assert.equal(f.prediction.accept(), true);
  assert.deepEqual(f.inserted, ['再跑一遍测试喵']);
  assert.equal(f.prediction.accept(), false);
  await f.prediction.completed('turn-1', []);
  assert.equal(f.calls(), 1);
});

for (const mutation of [
  current => { current.empty = false; current.revision++; },
  current => { current.session = 'session-b'; },
  current => { current.editable = false; },
  current => { current.idle = false; },
]) test('native state changes discard a late response', async () => {
  const f = fixture();
  const work = f.prediction.completed('turn-1', []);
  await flush();
  mutation(f.current);
  f.prediction.validate();
  assert.equal(f.signal().aborted, true);
  f.resolve({ suggestion: 'stale' });
  await work;
  assert.equal(f.prediction.read().kind, 'hidden');
  assert.equal(f.prediction.accept(), false);
  assert.deepEqual(f.inserted, []);
});

test('dispose and deadline abort network and animation without leaving loading visible', async () => {
  for (const dispose of [false, true]) {
    const f = fixture();
    const work = f.prediction.completed('turn-1', []);
    await flush();
    if (dispose) f.prediction.dispose(); else for (const expire of f.timeouts) expire();
    assert.equal(f.signal().aborted, true);
    f.resolve({ suggestion: 'late' });
    await work;
    assert.equal(f.intervals.size, 0);
    assert.equal(f.timeouts.size, 0);
    assert.equal(f.prediction.read().kind, 'hidden');
  }
});

test('malformed, multiline and control-character predictions never enter the native draft', async () => {
  for (const suggestion of [null, '', 'x\ny', '\x1b[31mred', 'a'.repeat(241), ' leading']) {
    const f = fixture();
    const work = f.prediction.completed('turn-1', []);
    await flush();
    f.resolve({ suggestion });
    await work;
    assert.equal(f.prediction.accept(), false);
    assert.equal(f.prediction.read().kind, 'hidden');
  }
});

test('context is bounded in Unicode characters without voice rewriting', () => {
  const text = '喵😺'.repeat(2000);
  const result = conversation(Array.from({ length: 40 }, () => ({ role: 'user', text })));
  assert.equal(result.length, 4);
  assert.equal(result.reduce((sum, item) => sum + Array.from(item.text).length, 0), 12000);
  assert.ok(result[0].text.startsWith('喵😺'));
  assert.ok(result[0].text.endsWith('喵😺'));
  assert.ok(result[0].text.includes('[... omitted ...]'));
  assert.deepEqual(conversation([{ role: 'tool', text: 'ignore' }, { role: 'user', text: '好耶！' }]),
    [{ role: 'user', text: '好耶！' }]);
});

test('a tool-heavy turn retains the latest nonempty user and original message order', () => {
  const messages = [{ role: 'user', text: '先测中文喵' },
    ...Array.from({ length: 40 }, (_, index) => ({ role: 'assistant', text: `${index}:` + '中'.repeat(4000) })),
    { role: 'user', text: ' '.repeat(5000) }, { role: 'tool', text: 'private' }];
  const result = conversation(messages);
  assert.equal(result[0].text, '先测中文喵');
  assert.equal(result.filter(message => message.role === 'user').length, 1);
  assert.ok(result.at(-1).text.startsWith('39:'));
  const positions = result.slice(1).map(message => Number.parseInt(message.text, 10));
  assert.deepEqual(positions, [...positions].sort((a, b) => a - b));
  assert.ok(result.length <= 24);
  assert.ok(result.reduce((sum, message) => sum + [...message.text].length, 0) <= 12000);
});

test('long messages keep final corrections with bounded character and block reads', () => {
  const original = '开始测中文' + '😺'.repeat(1_000_000) + '最后只改侧边 tab';
  const iterate = String.prototype[Symbol.iterator];
  let characters = 0;
  String.prototype[Symbol.iterator] = function* () {
    for (const char of iterate.call(this)) { characters++; yield char; }
  };
  let result;
  try { result = conversation([{ role: 'user', text: original }]); }
  finally { String.prototype[Symbol.iterator] = iterate; }
  assert.ok(result[0].text.startsWith('开始测中文'));
  assert.ok(result[0].text.endsWith('最后只改侧边 tab'));
  assert.equal([...result[0].text].length, 3000);
  assert.ok(characters < 10_000, `read ${characters} characters of a giant message`);
  const parts = [{ text: '首部' + '😺'.repeat(5000) },
    { get text() { assert.fail('must not read a discarded middle block'); } },
    { text: '😺'.repeat(5000) + '尾部纠正' }];
  const excerpt = textExcerpt(parts, part => part.text);
  assert.ok(excerpt.startsWith('首部'));
  assert.ok(excerpt.endsWith('尾部纠正'));
  assert.equal([...excerpt].length, 3000);
  assert.equal(textExcerpt(['一😺', '二']), '一😺\n二');
});

test('native extraction never opens discarded historical text', () => {
  const history = Array.from({ length: 1000 }, () => ({ role: 'assistant',
    get text() { assert.fail('discarded history must not be materialized'); } }));
  const recent = [{ role: 'user', text: '原话喵' },
    ...Array.from({ length: 4 }, () => ({ role: 'assistant', text: '中'.repeat(4000) }))];
  assert.equal(conversation([...history, ...recent])[0].text, '原话喵');
});

test('transport uses authenticated loopback HTTP and bounds its response', async t => {
  const requests = [];
  const server = createServer(async (req, res) => {
    requests.push({ url: req.url, auth: req.headers.authorization });
    if (req.url.endsWith('/config')) { res.end(JSON.stringify({ enabled: true, loading_text: 'sol predicting' })); return; }
    let body = '';
    for await (const chunk of req) body += chunk;
    const input = JSON.parse(body);
    res.end(JSON.stringify({ suggestion: input.large ? 'a'.repeat(9000) : '喵，测试' }));
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(() => { server.closeAllConnections(); server.close(); });
  const env = { PEBREL_PREDICTION_ENDPOINT: `http://127.0.0.1:${server.address().port}`,
    PEBREL_PREDICTION_TOKEN: 'test-only-token' };
  const transport = createTransport(env);
  const signal = new AbortController().signal;
  assert.equal((await transport.config(signal)).enabled, true);
  assert.equal((await transport.predict({}, signal)).suggestion, '喵，测试');
  assert.equal(requests[0].auth, 'Bearer test-only-token');
  await assert.rejects(transport.predict({ large: true }, signal), /response too large/);
  await assert.rejects(transport.predict({ text: 'x'.repeat(128 * 1024) }, signal), /request too large/);
  for (const url of ['https://example.com', 'http://localhost', 'http://user@127.0.0.1', 'http://127.0.0.1/path']) {
    assert.equal(createTransport({ ...env, PEBREL_PREDICTION_ENDPOINT: url }), undefined);
  }
});
