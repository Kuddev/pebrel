import assert from "node:assert/strict";
import { test } from "node:test";

let moduleId = 0;
const deferred = () => {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
};
const response = value => ({ ok: true, status: 200, text: JSON.stringify(value) });
const flush = async () => { for (let index = 0; index < 40; index += 1) await Promise.resolve(); };

async function fixture(options = {}) {
  const { register } = await import(`../hooks/prompt.js?test=${moduleId++}`);
  const hooks = new Map();
  const timers = new Set();
  const requests = [];
  const suggestions = [];
  const pending = deferred();
  let now = 0;
  let draft = options.draft ?? "";
  let session = "session-one";
  let messages = options.messages ?? [
    { role: "user", text: "喵，帮我把空输入框补齐好嘛～", toolResults: [] },
    { role: "assistant", text: "已实现，待验证。", toolUses: [] },
  ];
  const env = {
    PEBREL_PREDICTION_ENDPOINT: "http://127.0.0.1:12345",
    PEBREL_PREDICTION_TOKEN: "fixture-token",
    CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION: "false",
    ...options.env,
  };
  function timer(ms, fn, repeat) {
    const row = { at: now + ms, fn, repeat, ms };
    timers.add(row);
    return { cancel: () => timers.delete(row) };
  }
  const $ = {
    env: { get: async key => env[key] },
    session: { id: async () => session, messages: async () => messages },
    prompt: {
      read: async () => ({ text: draft, cursor: draft.length }),
      suggest: async value => {
        if (draft !== "") return { isShown: false };
        suggestions.push(value);
        if (options.suggestPending) await options.suggestPending.promise;
        return { isShown: true };
      },
    },
    clock: {
      after: (ms, fn) => timer(ms, fn, false),
      every: (ms, fn) => timer(ms, fn, true),
    },
    ui: { invalidate: () => {} },
    http: {
      fetch: async (url, init) => {
        requests.push({ url, ...init });
        if (url.endsWith("/config")) {
          return options.config ?? response({
            enabled: true, model: "gpt-6sol", language: "zh-CN", loading_text: "gpt-6sol正在预测",
          });
        }
        return pending.promise;
      },
    },
  };
  register((event, matcher, handler) => hooks.set(event, handler ?? matcher));
  const emit = (name, event, next = async value => value) => hooks.get(name)($, event, next);
  const render = (props = {}) => emit("ui.render", {
    surface: "terminal", component: "PromptHint",
    props: { isDraft: draft !== "", isWorking: false, hint: "? for shortcuts", ...props },
  });
  const advance = async ms => {
    const until = now + ms;
    while (true) {
      const next = [...timers].filter(row => row.at <= until).sort((a, b) => a.at - b.at)[0];
      if (!next) break;
      now = next.at;
      if (next.repeat) next.at += next.ms;
      else timers.delete(next);
      void next.fn();
      await flush();
    }
    now = until;
    await flush();
  };
  const finish = (fields = {}) => emit("turn.complete", {
    turnId: "turn-one", reason: "answer", answer: "已完成", isAborted: false, ...fields,
  });
  await emit("session.start", { isInteractive: true, surface: "terminal" });
  await render();
  return {
    $, emit, render, advance, finish, requests, suggestions, pending, timers,
    setDraft: value => { draft = value; },
    setSession: value => { session = value; },
    setMessages: value => { messages = value; },
  };
}

test("successful main turn uses native suggestion and noneditable cycling hint", async () => {
  const f = await fixture();
  await f.emit("turn.start", { turnId: "turn-one", text: "继续喵" });
  await f.finish();
  assert.equal(f.requests.length, 0, "completion hook does not await prediction");
  await f.advance(0);
  assert.equal(f.requests.length, 2);
  assert.equal((await f.render()).props.tail, "gpt-6sol正在预测.");
  await f.advance(400);
  assert.equal((await f.render()).props.tail, "gpt-6sol正在预测..");
  await f.advance(400);
  assert.equal((await f.render()).props.tail, "gpt-6sol正在预测...");
  await f.advance(400);
  assert.equal((await f.render({ tail: "existing" })).props.tail, "existing gpt-6sol正在预测.");
  assert.equal(f.suggestions.length, 0, "loading is never a Tab-acceptable suggestion");
  assert.equal((await f.render({ isDraft: true })).props.tail, undefined);
  assert.equal((await f.render({ isWorking: true })).props.tail, undefined);
  const post = f.requests[1];
  assert.equal(post.headers.Authorization, "Bearer fixture-token");
  assert.equal(post.headers["Content-Type"], "application/json");
  const body = JSON.parse(post.body);
  assert.equal(body.agent, "claude");
  assert.equal(body.session, "session-one");
  assert.equal(body.turn, "turn-one");
  assert.ok(Number.isInteger(body.revision));
  assert.deepEqual(body.messages, [
    { role: "user", text: "喵，帮我把空输入框补齐好嘛～" },
    { role: "assistant", text: "已实现，待验证。" },
  ]);
  f.pending.resolve(response({ suggestion: "那就跑一轮真机测试喵～", model: "gpt-6sol" }));
  await flush();
  assert.deepEqual(f.suggestions, [{ text: "那就跑一轮真机测试喵～" }]);
  assert.equal((await f.render()).props.tail, undefined);
  assert.equal(f.timers.size, 0);
});

test("native suggestion handoff hides loading before the native promise resolves", async () => {
  const suggestPending = deferred();
  const f = await fixture({ suggestPending });
  await f.finish();
  await f.advance(0);
  f.pending.resolve(response({ suggestion: "继续验证喵", model: "gpt-6sol" }));
  await flush();
  assert.deepEqual(f.suggestions, [{ text: "继续验证喵" }]);
  assert.equal((await f.render()).props.tail, undefined);
  assert.equal(f.timers.size, 0);
  suggestPending.resolve();
  await flush();
});

test("nonempty, failed, interrupted, blank and subagent completions never request", async () => {
  for (const fields of [
    { reason: "error" }, { reason: "refusal" }, { reason: "aborted" },
    { isAborted: true }, { answer: "" }, { agentId: "child" },
  ]) {
    const f = await fixture();
    await f.finish(fields);
    await f.advance(0);
    assert.equal(f.requests.length, 0);
  }
  for (const draft of ["自己的草稿", " "]) {
    const f = await fixture({ draft });
    await f.finish();
    await f.advance(0);
    assert.equal(f.requests.length, 0);
  }
});

test("requires an interactive mounted terminal composer", async () => {
  for (const start of [
    { isInteractive: false, surface: null },
    { isInteractive: true, surface: "desktop" },
    { isInteractive: true, surface: "terminal" },
  ]) {
    const f = await fixture();
    await f.emit("session.start", start);
    await f.finish();
    await f.advance(0);
    assert.equal(f.requests.length, 0);
  }
});

test("subagent activity and late old completion do not finish the active main turn", async () => {
  const f = await fixture();
  await f.emit("turn.start", { turnId: "turn-two" });
  await f.emit("turn.start", { turnId: "child", agentId: "child" });
  await f.finish({ turnId: "child", agentId: "child" });
  await f.finish({ turnId: "turn-one" });
  await f.advance(0);
  assert.equal(f.requests.length, 0);
  await f.finish({ turnId: "turn-two" });
  await f.advance(0);
  f.pending.resolve(response({ suggestion: "继续这轮测试" }));
  await flush();
  assert.equal(f.suggestions.length, 1);
  assert.equal(JSON.parse(f.requests[1].body).turn, "turn-two");
});

test("turn is handled once even after dismissal and clearing the composer", async () => {
  const f = await fixture();
  await f.finish();
  await f.advance(0);
  await f.emit("prompt.change", { text: "", origin: { kind: "clear" } });
  f.pending.resolve(response({ suggestion: "不能回来" }));
  await flush();
  await f.finish();
  await f.advance(0);
  assert.equal(f.requests.length, 2);
  assert.equal(f.suggestions.length, 0);
  assert.equal(f.timers.size, 0);
});

test("pre-edit cancellation wins before native draft commit", async () => {
  const f = await fixture();
  await f.finish();
  await f.advance(0);
  await f.emit("prompt.edit", { text: "a" }, async () => {
    f.pending.resolve(response({ suggestion: "迟到的建议" }));
    await flush();
    assert.equal(f.suggestions.length, 0);
    f.setDraft("自己的草稿");
    return { text: "自己的草稿", cursor: 5 };
  });
  assert.equal((await f.render()).props.tail, undefined);
  assert.equal(f.timers.size, 0);
});

test("session changes, ends, new turns and deadlines reject late results", async () => {
  for (const cause of ["identity", "end", "turn", "deadline"]) {
    const f = await fixture();
    await f.finish();
    await f.advance(0);
    if (cause === "identity") f.setSession("another-session");
    if (cause === "end") await f.emit("session.end", { reason: "prompt_input_exit" });
    if (cause === "turn") await f.emit("turn.start", { turnId: "turn-two" });
    if (cause === "deadline") await f.advance(10000);
    f.pending.resolve(response({ suggestion: "过期的建议" }));
    await flush();
    assert.equal(f.suggestions.length, 0, cause);
    assert.equal((await f.render()).props.tail, undefined, cause);
    assert.equal(f.timers.size, 0, cause);
  }
});

test("clear/resume can predict in the next native session without session.start", async () => {
  for (const reason of ["clear", "resume"]) {
    const f = await fixture();
    await f.emit("session.end", { reason });
    f.setSession("session-two");
    await f.render();
    await f.emit("turn.start", { turnId: "turn-two" });
    await f.finish({ turnId: "turn-two" });
    await f.advance(0);
    f.pending.resolve(response({ suggestion: "继续验证" }));
    await flush();
    assert.deepEqual(f.suggestions, [{ text: "继续验证" }]);
    assert.equal(JSON.parse(f.requests[1].body).session, "session-two");
  }
});

test("null, invalid, rejected and unavailable results clean up loading", async () => {
  for (const result of [
    response({ suggestion: null }), response({ suggestion: "" }),
    response({ suggestion: "line\nbreak" }), response({ suggestion: "x".repeat(241) }),
    { ok: false, status: 503, text: "" }, { ok: true, text: "not json" }, null,
  ]) {
    const f = await fixture();
    await f.finish();
    await f.advance(0);
    if (result === null) f.pending.reject(new Error("offline"));
    else f.pending.resolve(result);
    await flush();
    assert.equal(f.suggestions.length, 0);
    assert.equal((await f.render()).props.tail, undefined);
    assert.equal(f.timers.size, 0);
  }
});

test("stale config and disabled config never launch inference", async () => {
  const config = deferred();
  const f = await fixture({ config: config.promise });
  await f.finish();
  await f.advance(0);
  await f.emit("prompt.edit", {});
  config.resolve(response({ enabled: true, loading_text: "gpt-6sol正在预测" }));
  await flush();
  assert.equal(f.requests.length, 1);
  assert.equal(f.timers.size, 0);
  const disabled = await fixture({ config: response({ enabled: false }) });
  await disabled.finish();
  await disabled.advance(0);
  assert.equal(disabled.requests.length, 1);
  assert.equal(disabled.timers.size, 0);
});

test("valid 256-character model names predict while config responses remain bounded in UTF-8 bytes", async () => {
  for (const unit of ["m", "模", "😺"]) {
    const loading = unit.repeat(256) + " 正在预测";
    const f = await fixture({ config: response({ enabled: true, loading_text: loading }) });
    await f.finish(); await f.advance(0);
    assert.equal(f.requests.length, 2);
    assert.equal((await f.render()).props.tail, loading + ".");
    f.pending.resolve(response({ suggestion: "继续测试" })); await flush();
    assert.deepEqual(f.suggestions, [{ text: "继续测试" }]);
  }
  for (const loading of ["x".repeat(8193), "中".repeat(3000)]) {
    const f = await fixture({ config: response({ enabled: true, loading_text: loading }) });
    await f.finish(); await f.advance(0);
    assert.equal(f.requests.length, 1);
    assert.equal(f.timers.size, 0);
  }
});

test("requires process-only native suppression and authenticated loopback bridge", async () => {
  for (const env of [
    { CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION: undefined },
    { CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION: "true" },
    { PEBREL_PREDICTION_ENDPOINT: "https://example.com:443" },
    { PEBREL_PREDICTION_ENDPOINT: "http://127.0.0.1:65536" },
    { PEBREL_PREDICTION_ENDPOINT: "http://127.0.0.1:12/elsewhere" },
    { PEBREL_PREDICTION_TOKEN: "" },
  ]) {
    const f = await fixture({ env });
    await f.finish();
    await f.advance(0);
    assert.equal(f.requests.length, 0);
    assert.equal(f.timers.size, 0);
  }
});

test("native context is bounded, text-only and keeps both ends of long answers", async () => {
  const messages = Array.from({ length: 100 }, (_, index) => ({
    role: index % 2 ? "assistant" : "user", text: `row-${index} ` + "😀".repeat(2000),
    toolUses: [{ secret: "never transmit" }],
  }));
  messages.push({ role: "tool", text: "never transmit tool output" });
  const f = await fixture({ messages });
  await f.finish();
  await f.advance(0);
  const body = JSON.parse(f.requests[1].body);
  assert.ok(body.messages.length <= 24);
  assert.ok(body.messages.reduce((sum, message) => sum + message.text.length, 0) <= 12000);
  assert.ok(body.messages.every(message => message.text.length <= 3000));
  assert.ok(body.messages.at(-1).text.startsWith("row-99"));
  assert.ok(body.messages.at(-1).text.endsWith("😀"));
  assert.ok(body.messages.at(-1).text.includes("[... omitted ...]"));
  assert.ok(!f.requests[1].body.includes("never transmit"));
  f.pending.resolve(response({ suggestion: null }));
  await flush();
});

test("long assistant-only runs keep the latest nonempty user without opening old text", async () => {
  const messages = [{ role: "user", get text() { assert.fail("old text must not be read"); } },
    { role: "user", text: "先测中文" + "😺".repeat(200_000) + "最后只改侧边 tab 喵" },
    ...Array.from({ length: 40 }, (_, index) => ({ role: "assistant", text: `${index}:` + "中".repeat(4000) })),
    { role: "user", text: " ".repeat(5000) }];
  const f = await fixture({ messages });
  await f.finish(); await f.advance(0);
  assert.equal(f.requests.length, 2);
  const context = JSON.parse(f.requests[1].body).messages;
  assert.equal(context[0].role, "user");
  assert.ok(context[0].text.startsWith("先测中文"));
  assert.ok(context[0].text.endsWith("最后只改侧边 tab 喵"));
  assert.ok(context.at(-1).text.startsWith("39:"));
  assert.ok(context.reduce((sum, message) => sum + message.text.length, 0) <= 12000);
  assert.ok(context.length <= 24);
  f.pending.resolve(response({ suggestion: null })); await flush();
});
