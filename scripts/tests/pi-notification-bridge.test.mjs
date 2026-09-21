import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { stripTypeScriptTypes } from "node:module";
import { openSync, readSync, closeSync } from "node:fs";
import { randomUUID } from "node:crypto";
import { test } from "node:test";
import { runInNewContext } from "node:vm";

// Execute the shipped bridge with a fake Pi event bus and child process sink.
// No installed extensions, user sessions, or actual helper processes are touched.
const embedded = readFileSync(new URL("../../nebula_app/res/hooks/pi.ts", import.meta.url), "utf8");
const code = stripTypeScriptTypes(embedded)
  .replace('import { spawn } from "node:child_process";', "")
  .replace('import { openSync, readSync, closeSync } from "node:fs";', "")
  .replace('import { randomUUID } from "node:crypto";', "")
  .replace('await import(name)', 'await loadPiModule(name)')
  .replace("export default async function", "async function register");

async function bridge() {
  const handlers = new Map();
  const sent = [];
  const ctx = { cwd: "/project", sessionManager: { getSessionId: () => "session-1" } };
  await runInNewContext(`${code}\nregister(pi);`, {
    pi: { on: (name, handler) => handlers.set(name, handler) },
    process: { pid: 42, env: { PEBREL_HOOK_EXE: "fake-helper" } },
    openSync, readSync, closeSync, randomUUID, Buffer,
    loadPiModule: async () => ({ VERSION: "0.85.1" }),
    spawn: (_exe, args) => {
      sent.push(JSON.parse(args[1]));
      return { on() {}, unref() {} };
    },
  });
  return {
    sent,
    emit: (name, event = {}) => handlers.get(name)(event, ctx),
    settle: () => handlers.get("agent_settled")({}, ctx),
  };
}

const assistant = (stopReason, errorMessage) => ({ role: "assistant", stopReason, errorMessage });

test("API overload preserves the failure instead of sending a bare completion", async () => {
  const b = await bridge();
  await b.emit("agent_start");
  await b.emit("agent_end", { messages: [assistant("error", "Our servers are currently overloaded.")] });
  assert.equal(b.sent.length, 1, "an attempt is not a settled turn");
  await b.settle();
  assert.equal(b.sent.length, 2);
  assert.equal(b.sent[1].kind, "done");
  assert.equal(b.sent[1].stop_reason, "error");
  assert.equal(b.sent[1].message, undefined);
  assert.equal(b.sent[1].session_id, "session-1");
});

test("a successful retry overrides an earlier error in the same turn", async () => {
  const b = await bridge();
  await b.emit("agent_start");
  await b.emit("agent_end", { messages: [assistant("error", "overload"), assistant("stop")] });
  await b.settle();
  assert.equal(b.sent[1].stop_reason, "stop");
  assert.equal(b.sent[1].message, undefined);
});

test("a failed tool is not an assistant request failure", async () => {
  const b = await bridge();
  await b.emit("agent_start");
  await b.emit("agent_end", { messages: [{ role: "toolResult", isError: true }, assistant("stop")] });
  await b.settle();
  assert.equal(b.sent[1].stop_reason, "stop");
});

test("cancellation and length exhaustion retain their distinct outcomes", async () => {
  for (const reason of ["aborted", "length", "toolUse"]) {
    const b = await bridge();
    await b.emit("agent_start");
    await b.emit("agent_end", { messages: [assistant(reason)] });
    await b.settle();
    assert.equal(b.sent[1].stop_reason, reason);
  }
});

test("missing assistant metadata never fabricates success", async () => {
  const b = await bridge();
  await b.emit("agent_start");
  await b.emit("agent_end", { messages: [] });
  await b.settle();
  assert.equal(b.sent[1].stop_reason, "unknown");
});

test("duplicate agent_end and agent_settled emit only once until a new turn starts", async () => {
  const b = await bridge();
  const event = { messages: [assistant("error", "overload")] };
  await b.emit("agent_start");
  await b.emit("agent_end", event);
  await b.emit("agent_end", event);
  await b.settle();
  await b.settle();
  assert.equal(b.sent.length, 2);
  await b.emit("agent_start");
  await b.emit("agent_end", event);
  await b.settle();
  assert.equal(b.sent.length, 4);
  assert.notEqual(b.sent[1].event_id, b.sent[3].event_id);
  assert.ok(BigInt(b.sent[3].bridge_sequence) > BigInt(b.sent[1].bridge_sequence));
});

test("oversized provider errors are omitted before spawning the helper", async () => {
  const b = await bridge();
  await b.emit("agent_start");
  await b.emit("agent_end", { messages: [assistant("error", "x".repeat(10_000))] });
  await b.settle();
  assert.equal(b.sent[1].stop_reason, "error");
  assert.equal(b.sent[1].message, undefined);
});
