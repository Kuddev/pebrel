import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { runInNewContext } from "node:vm";

// Execute the installed source with a provider event bus and a fake helper.
// The tests do not install plugins or contact a running Agent.
const embedded = readFileSync(new URL("../../nebula_app/res/hooks/opencode.js", import.meta.url), "utf8");
const code = embedded
  .replace('import { execFile } from "node:child_process"', "")
  .replace("export default", "const plugin =");

async function bridge(enabled = true) {
  const sent = [];
  const handlers = await runInNewContext(`${code}\nplugin.server({ directory: '/project' });`, {
    execFile(_helper, args, _options, done) { sent.push(JSON.parse(args[1])); done(null); },
    process: { env: enabled ? { PEBREL_HOOK_EXE: "fake-helper" } : {} },
    AbortController, setTimeout, clearTimeout,
  });
  const flush = () => new Promise(setImmediate);
  return {
    sent,
    async emit(type, properties) { await handlers.event({ event: { type, properties } }); await flush(); },
    async permission(input) { await handlers["permission.ask"](input); await flush(); },
    handlers,
  };
}

function prompt(sessionID, id) {
  return { info: { role: "user", sessionID, id } };
}

test("permission waits preserve completion through tool resumption and idle", async () => {
  const b = await bridge();
  await b.emit("message.updated", prompt("main", "user-1"));
  await b.permission({ sessionID: "main", id: "request-1", permission: "bash" });
  await b.emit("tool.execute.after", { sessionID: "main" });
  await b.emit("session.idle", { sessionID: "main" });
  await b.emit("session.idle", { sessionID: "main" });
  assert.deepEqual(b.sent.map(event => event.kind), ["session-start", "prompt", "attention", "tool-complete", "done"]);
  assert.ok(b.sent.every(event => event.session_id === "main"));
});

test("nested completions cannot consume the primary session's pending completion", async () => {
  const b = await bridge();
  await b.emit("message.updated", prompt("main", "user-1"));
  await b.emit("message.updated", prompt("child", "user-2"));
  await b.emit("session.idle", { sessionID: "child" });
  await b.emit("session.idle", { sessionID: "main" });
  assert.deepEqual(b.sent.filter(event => event.kind === "done").map(event => event.session_id), ["child", "main"]);
});

test("user-message deduplication belongs to a session", async () => {
  const b = await bridge();
  await b.emit("message.updated", prompt("main", "user-1"));
  await b.emit("message.updated", prompt("child", "user-1"));
  await b.emit("message.updated", prompt("main", "user-1"));
  await b.emit("message.updated", prompt("child", "user-1"));
  assert.deepEqual(b.sent.filter(event => event.kind === "prompt").map(event => event.session_id), ["main", "child"]);
});

test("startup idle does not fabricate a completed turn", async () => {
  const b = await bridge();
  await b.emit("session.idle", { sessionID: "main" });
  assert.deepEqual(b.sent.map(event => event.kind), ["session-start"]);
});

test("the installed plugin is inert outside the terminal environment", async () => {
  const b = await bridge(false);
  assert.deepEqual(Object.keys(b.handlers), []);
  assert.deepEqual(b.sent, []);
});

// Import the installed module as ESM: evaluating a stripped V1 function alone
// cannot catch the missing-default-export failure from OpenCode's V2 loader.
test("the installed ESM has one default definition for V1.18.29+ and V2", async () => {
  const module = await import(`data:text/javascript;base64,${Buffer.from(embedded).toString("base64")}`);
  assert.deepEqual(Object.keys(module), ["default"]);
  assert.equal(module.default.id, "pebrel");
  assert.equal(typeof module.default.server, "function");
  assert.equal(typeof module.default.setup, "function");
});

async function v2({ env = { PEBREL_HOOK_EXE: "fake helper" }, execute, timer = setTimeout } = {}) {
  const sent = [];
  const calls = [];
  const plugin = runInNewContext(`${code}\nplugin;`, {
    process: { env },
    AbortController, setTimeout: timer, clearTimeout,
    execFile(helper, args, options, done) {
      calls.push({ helper, args, options });
      sent.push(JSON.parse(args[1]));
      if (execute) execute(options, done, calls.length);
      else done(null);
    },
  });
  const pending = [];
  let wake = () => {};
  let failure;
  let subscriptions = 0;
  let closed = 0;
  const cleanup = await plugin.setup({
    location: { directory: "/project" },
    event: {
      async *subscribe({ signal }) {
        subscriptions++;
        const abort = () => wake();
        signal.addEventListener("abort", abort, { once: true });
        try {
          while (!signal.aborted) {
            if (failure) throw failure;
            if (pending.length) yield pending.shift();
            else await new Promise(resolve => { wake = resolve; });
          }
        } finally {
          signal.removeEventListener("abort", abort);
          closed++;
        }
      },
    },
  });
  let id = 0;
  return {
    sent, calls, cleanup,
    get subscriptions() { return subscriptions; },
    get closed() { return closed; },
    async emit(type, data, extra = {}) {
      pending.push({ id: `evt_${++id}`, type, data, location: { directory: "/project" }, ...extra });
      wake();
      await new Promise(setImmediate);
    },
    async fail() { failure = new Error("subscription closed"); wake(); await new Promise(setImmediate); },
  };
}

test("V2 durable execution events preserve permission context, ordering and completion", async (t) => {
  const b = await v2();
  t.after(b.cleanup);
  await b.emit("session.execution.started", { sessionID: "main" });
  const request = { sessionID: "main", id: "per_1", action: "bash", resources: ["echo 'hello'"], message: "Run shell?" };
  await b.emit("permission.asked", request);
  await b.emit("session.tool.success", { sessionID: "main" });
  await b.emit("session.execution.succeeded", { sessionID: "main" });
  await b.emit("session.execution.succeeded", { sessionID: "main" });
  assert.deepEqual(b.sent.map(e => e.kind), ["session-start", "prompt", "attention", "tool-complete", "done"]);
  const attention = b.sent[2];
  assert.deepEqual(attention.context, request);
  assert.equal(attention.permission_or_tool, "bash");
  assert.equal(attention.event_id, "main:permission:per_1");
  assert.equal(attention.message, "Run shell?");
  assert(b.sent.every(e => e.cwd === "/project" && e.session_id === "main"));
  assert(b.sent.every((e, i) => !i || BigInt(e.bridge_sequence) > BigInt(b.sent[i - 1].bridge_sequence)));
  assert(b.calls.every(c => c.helper === "fake helper" && c.args[0] === "opencode"));
});

test("V2 ignores other locations, startup idle and non-waiting permission events", async (t) => {
  const b = await v2();
  t.after(b.cleanup);
  await b.emit("session.execution.started", { sessionID: "other" }, { location: { directory: "/other" } });
  await b.emit("session.status", { sessionID: "main", status: { type: "idle" } });
  await b.emit("session.idle", { sessionID: "main" });
  await b.emit("permission.replied", { sessionID: "main", requestID: "per_1", reply: "once" });
  assert.deepEqual(b.sent, []);
});

test("V2 interleaved sessions and repeated starts do not lose either completion", async (t) => {
  const b = await v2();
  t.after(b.cleanup);
  await b.emit("session.execution.started", { sessionID: "main" }, { id: "evt_main" });
  await b.emit("session.execution.started", { sessionID: "main" }, { id: "evt_main" });
  await b.emit("session.execution.started", { sessionID: "child" });
  await b.emit("session.execution.succeeded", { sessionID: "child" });
  await b.emit("session.execution.succeeded", { sessionID: "main" });
  assert.deepEqual(b.sent.filter(e => e.kind === "prompt").map(e => e.session_id), ["main", "child"]);
  assert.deepEqual(b.sent.filter(e => e.kind === "done").map(e => e.session_id), ["child", "main"]);
});

for (const terminal of ["failed", "interrupted"]) {
  test(`V2 ${terminal} executions stop the spinner, and deletion ends the session`, async (t) => {
    const b = await v2();
    t.after(b.cleanup);
    await b.emit("session.execution.started", { sessionID: "main" });
    await b.emit("session.tool.failed", { sessionID: "main" });
    await b.emit(`session.execution.${terminal}`, { sessionID: "main" });
    await b.emit("session.deleted", { sessionID: "main" });
    assert.deepEqual(b.sent.map(e => e.kind), ["session-start", "prompt", "tool-complete", "done", "session-end"]);
  });
}

test("V2 cleanup cancels the subscription, active helper and queued deliveries", async () => {
  const b = await v2({ execute(options, done) {
    options.signal.addEventListener("abort", () => done(new Error("aborted")), { once: true });
  } });
  await b.emit("session.execution.started", { sessionID: "main" });
  assert.equal(b.calls.length, 1); // The prompt waits behind session-start.
  const options = b.calls[0].options;
  assert.equal(options.timeout, 3000);
  assert.equal(options.killSignal, "SIGKILL");
  assert.equal(options.windowsHide, true);
  assert.equal(options.shell, undefined);
  await b.cleanup();
  assert.equal(options.signal.aborted, true);
  assert.equal(b.closed, 1);
  assert.equal(b.calls.length, 1);
  await b.emit("session.execution.succeeded", { sessionID: "main" });
  assert.equal(b.calls.length, 1);
});

test("helper failures do not poison the serialized queue or the Agent", async (t) => {
  const b = await v2({ execute(_options, done, count) {
    if (count === 1) throw new Error("spawn failed");
    done(count === 2 ? new Error("timed out") : null);
  } });
  t.after(b.cleanup);
  await b.emit("session.execution.started", { sessionID: "main" });
  await b.emit("session.execution.succeeded", { sessionID: "main" });
  assert.deepEqual(b.sent.map(e => e.kind), ["session-start", "prompt", "done"]);
});

test("V2 subscription rejection is handled and cleanup still completes", async () => {
  const b = await v2();
  await b.fail();
  await b.cleanup();
  assert.equal(b.closed, 1);
});

test("V2 is inert outside Pebrel and retains the legacy helper environment alias", async (t) => {
  const inert = await v2({ env: {} });
  assert.equal(inert.subscriptions, 0);
  assert.equal(inert.cleanup, undefined);
  const legacy = await v2({ env: { NEBULA_HOOK_EXE: "legacy helper" } });
  t.after(legacy.cleanup);
  await legacy.emit("session.execution.started", { sessionID: "main" });
  assert(legacy.calls.every(c => c.helper === "legacy helper"));
});

// Bus.Subscribe routes these by session ownership without changing the public
// envelope (OpenCode v2.0.18 packages/core/src/bus.ts).
test("V2 unlocated durable session events still produce a complete turn", async (t) => {
  const b = await v2();
  t.after(b.cleanup);
  await b.emit("session.execution.started", { sessionID: "main" }, { location: undefined });
  await b.emit("session.execution.succeeded", { sessionID: "main" }, { location: undefined });
  assert.deepEqual(b.sent.map(e => e.kind), ["session-start", "prompt", "done"]);
});

test("the watchdog releases the queue even when a helper never calls back", async (t) => {
  let release;
  const b = await v2({
    execute(_options, done, count) { if (count > 1) done(null); },
    timer(resolve) { release = resolve; return 0; },
  });
  t.after(b.cleanup);
  await b.emit("session.execution.started", { sessionID: "main" });
  assert.equal(b.sent.length, 1);
  release();
  await b.emit("session.execution.succeeded", { sessionID: "main" });
  assert.deepEqual(b.sent.map(e => e.kind), ["session-start", "prompt", "done"]);
});

test("the installed module delivers literal arguments through a real helper process", (t) => {
  const directory = mkdtempSync(join(tmpdir(), "pebrel hook "));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  // Node/Bun acts as the helper executable; its first argument is the opencode
  // script. No shell, global environment mutation or running Agent is needed.
  writeFileSync(join(directory, "opencode"), `
    const { appendFileSync } = require("node:fs");
    appendFileSync("delivered.jsonl", process.argv[2] + "\\n");
    console.log("helper output must remain quiet");
  `);
  writeFileSync(join(directory, "worker.mjs"), `
    import assert from "node:assert/strict";
    import { existsSync, readFileSync } from "node:fs";
    import { setTimeout as delay } from "node:timers/promises";
    const { default: plugin } = await import(process.argv[2]);
    const bridge = await plugin.server({ directory: process.cwd() });
    const message = 'space "quote" $HOME & ; | < >';
    try {
      await bridge.event({ event: { type: "message.updated", properties: {
        info: { role: "user", sessionID: "main", id: "user-1" }
      } } });
      await bridge["permission.ask"]({ sessionID: "main", id: "per_1", permission: "bash", message });
      await bridge.event({ event: { type: "session.idle", properties: { sessionID: "main" } } });
      let messages = [];
      const deadline = Date.now() + 10000;
      while (Date.now() < deadline) {
        if (existsSync("delivered.jsonl")) {
          messages = readFileSync("delivered.jsonl", "utf8").trim().split("\\n").map(JSON.parse);
          if (messages.length === 4) break;
        }
        await delay(10);
      }
      assert.deepEqual(messages.map(e => e.kind), ["session-start", "prompt", "attention", "done"]);
      assert.equal(messages[2].message, message);
    } finally { await bridge.dispose(); }
  `);
  const stdout = execFileSync(process.execPath, [join(directory, "worker.mjs"),
    new URL("../../nebula_app/res/hooks/opencode.js", import.meta.url).href], {
    cwd: directory,
    env: { ...process.env, PEBREL_HOOK_EXE: process.execPath },
    timeout: 15000,
    encoding: "utf8",
  });
  assert.equal(stdout, "");
});
