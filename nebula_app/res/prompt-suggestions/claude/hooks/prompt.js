// Claude Code function hooks own the composer. No screen parsing, draft writes,
// model overrides, transcript polling, or per-user style storage live here.
const state = {
  revision: 0,
  interactive: false,
  composer: false,
  busy: false,
  turn: null,
  handled: null,
  loading: null,
  dots: 1,
  tick: null,
  deadline: null,
  scheduled: null,
};

function stopLoading($) {
  state.tick?.cancel();
  state.deadline?.cancel();
  state.tick = state.deadline = null;
  if (state.loading !== null) {
    state.loading = null;
    $.ui.invalidate("ui.render");
  }
}

function invalidate($) {
  state.revision += 1;
  state.scheduled?.cancel();
  state.scheduled = null;
  stopLoading($);
}

function current(revision) {
  return state.revision === revision && state.interactive && state.composer && !state.busy;
}

function startLoading($, text, revision) {
  state.loading = text;
  state.dots = 1;
  $.ui.invalidate("ui.render");
  state.tick = $.clock.every(400, () => {
    if (!current(revision)) return stopLoading($);
    state.dots = state.dots % 3 + 1;
    $.ui.invalidate("ui.render");
  });
}

function boundedMessages(messages) {
  function excerpt(message, limit) {
    let text = message.text;
    if (typeof text !== "string" || !/\S/u.test(text)) return "";
    if (text.length > limit) {
      const gap = "\n[... omitted ...]\n";
      if (limit <= gap.length + 2) {
        text = text.slice(-limit).replace(/^[\uDC00-\uDFFF]/, "");
      } else {
        const headLength = Math.floor((limit - gap.length) / 2);
        const head = text.slice(0, headLength).replace(/[\uD800-\uDBFF]$/, "");
        const tail = text.slice(-(limit - gap.length - headLength)).replace(/^[\uDC00-\uDFFF]/, "");
        text = head + gap + tail;
      }
    }
    return text;
  }
  const result = [];
  let remaining = 12000;
  let userIndex = -1;
  // Reserve the latest user's wording even after a long, tool-heavy answer.
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    if (messages[index].role !== "user") continue;
    const text = excerpt(messages[index], 3000);
    if (text.trim() === "") continue;
    userIndex = index;
    result.push({ index, message: { role: "user", text } });
    remaining -= text.length;
    break;
  }
  for (let index = messages.length - 1; index >= 0 && remaining > 0 && result.length < 24; index -= 1) {
    if (index === userIndex) continue;
    const message = messages[index];
    if (message.role !== "user" && message.role !== "assistant") continue;
    const text = excerpt(message, Math.min(remaining, 3000));
    if (text.trim() === "") continue;
    remaining -= text.length;
    result.push({ index, message: { role: message.role, text } });
  }
  return result.sort((a, b) => a.index - b.index).map(entry => entry.message);
}

async function bridge($) {
  const [endpoint, token, nativeSuggestion] = await Promise.all([
    $.env.get("PEBREL_PREDICTION_ENDPOINT"),
    $.env.get("PEBREL_PREDICTION_TOKEN"),
    $.env.get("CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION"),
  ]);
  // Suppress the built-in inference at process launch, never via persisted
  // config.set or by discarding its already-paid result in prompt.suggest.
  if (nativeSuggestion !== "false" && nativeSuggestion !== "0") return null;
  const match = /^http:\/\/127\.0\.0\.1:(\d{1,5})$/.exec(endpoint ?? "");
  if (!match || Number(match[1]) < 1 || Number(match[1]) > 65535) return null;
  if (!token || token.length > 512 || /[\r\n]/.test(token)) return null;
  return { endpoint, headers: { Authorization: `Bearer ${token}` } };
}

function readJson(response) {
  if (!response.ok || typeof response.text !== "string" || response.text.length > 8192) {
    throw new Error("Invalid prediction bridge response");
  }
  let bytes = 0;
  for (const char of response.text) {
    const point = char.codePointAt(0);
    bytes += point <= 0x7f ? 1 : point <= 0x7ff ? 2 : point <= 0xffff ? 3 : 4;
    if (bytes > 8192) throw new Error("Invalid prediction bridge response");
  }
  return JSON.parse(response.text);
}

function candidate(value) {
  if (typeof value !== "string" || value.length > 1024) return null;
  if (/[\u0000-\u001F\u007F-\u009F\u2028\u2029]/.test(value)) return null;
  const text = value.trim();
  return text && [...text].length <= 240 ? text : null;
}

async function sameEmptyComposer($, revision, session) {
  if (!current(revision)) return false;
  const [id, draft] = await Promise.all([$.session.id(), $.prompt.read()]);
  return current(revision) && id === session && draft.text === "";
}

async function predict($, turn, revision) {
  try {
    if (!current(revision)) return;
    const [connection, session, draft] = await Promise.all([
      bridge($), $.session.id(), $.prompt.read(),
    ]);
    if (!connection || !current(revision) || draft.text !== "") return;
    const config = readJson(await $.http.fetch(
      `${connection.endpoint}/v1/prompt-suggestions/config`,
      { headers: connection.headers },
    ));
    // Settings own model-name validation; the complete response is bounded above.
    if (!config.enabled || typeof config.loading_text !== "string" ||
        config.loading_text.trim() === "" ||
        /[\u0000-\u001F\u007F-\u009F\u2028\u2029]/.test(config.loading_text)) return;
    if (!await sameEmptyComposer($, revision, session)) return;
    startLoading($, config.loading_text, revision);
    const messages = boundedMessages(await $.session.messages());
    if (!messages.some(message => message.role === "user")) return;
    if (!await sameEmptyComposer($, revision, session)) return;
    const response = readJson(await $.http.fetch(
      `${connection.endpoint}/v1/prompt-suggestions`,
      {
        method: "POST",
        headers: { ...connection.headers, "Content-Type": "application/json" },
        body: JSON.stringify({ agent: "claude", session, turn, revision, messages }),
      },
    ));
    const text = candidate(response.suggestion);
    if (text && await sameEmptyComposer($, revision, session)) {
      // Native suggest rechecks empty/not-working atomically when it lands.
      // It owns Tab/right-arrow acceptance and never submits the draft.
      stopLoading($);
      await $.prompt.suggest({ text });
    }
  } catch {
    // Predictions are optional; a failed bridge must not disrupt a main turn.
  } finally {
    if (state.revision === revision) stopLoading($);
  }
}

export function register(on) {
  on("session.start", ($, event, next) => {
    invalidate($);
    state.interactive = event.isInteractive && event.surface === "terminal";
    state.composer = state.busy = false;
    state.turn = state.handled = null;
    return next(event);
  });

  on("turn.start", ($, event, next) => {
    if (event.agentId === undefined) {
      invalidate($);
      state.busy = true;
      state.turn = event.turnId;
    }
    return next(event);
  });

  on("turn.complete", async ($, event, next) => {
    const result = await next(event);
    if (event.agentId !== undefined || (state.turn && state.turn !== event.turnId)) return result;
    state.busy = false;
    if (state.handled === event.turnId) return result;
    state.handled = event.turnId;
    invalidate($);
    if (event.reason !== "answer" || event.isAborted || !event.answer.trim()) return result;
    const revision = state.revision;
    // Leave the turn.complete stack before asking the composer. Awaiting the
    // bridge inside that hook would keep the main turn working unnecessarily.
    state.scheduled = $.clock.after(0, () => {
      state.scheduled = null;
      if (!current(revision)) return;
      // http.fetch has no AbortSignal. Bound UI lifetime and ignore late results;
      // the host owns the request/network deadline.
      state.deadline = $.clock.after(10000, () => {
        if (state.revision === revision) invalidate($);
      });
      return predict($, event.turnId, revision);
    });
    return result;
  });

  on("prompt.edit", ($, event, next) => {
    // This event is pre-edit; cancel before next instead of rereading old text.
    invalidate($);
    return next(event);
  });

  on("prompt.change", ($, event, next) => {
    invalidate($);
    return next(event);
  });

  on("session.end", ($, event, next) => {
    invalidate($);
    state.composer = state.busy = false;
    state.turn = state.handled = null;
    if (event.reason !== "clear" && event.reason !== "resume") state.interactive = false;
    return next(event);
  });

  on("ui.render", { component: "PromptHint" }, ($, event, next) => {
    if (event.surface !== "terminal") return next(event);
    state.composer = true;
    if (!state.loading || event.props.isDraft || event.props.isWorking) return next(event);
    const loading = `${state.loading}${".".repeat(state.dots)}`;
    const tail = event.props.tail ? `${event.props.tail} ${loading}` : loading;
    return next({ ...event, props: { ...event.props, tail } });
  });
}
