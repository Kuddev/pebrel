import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { runInNewContext } from "node:vm";

// Execute the installed source with a provider event bus and a fake helper.
// The tests do not install plugins or contact a running Agent.
const embedded = readFileSync(new URL("../../nebula_app/res/hooks/opencode.js", import.meta.url), "utf8");
const code = embedded.replace("export const PebrelNotify", "const PebrelNotify");

async function bridge(enabled = true) {
  const sent = [];
  const shell = (_strings, _helper, payload) => ({
    quiet() { return this; },
    nothrow() { sent.push(JSON.parse(payload)); return Promise.resolve(); },
  });
  const handlers = await runInNewContext(`${code}\nPebrelNotify({ $, directory: '/project' });`, {
    $: shell,
    process: { env: enabled ? { PEBREL_HOOK_EXE: "fake-helper" } : {} },
    // A successful fake helper always wins the watchdog race; do not create
    // real timers for each synthetic event.
    setTimeout: () => 0,
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
