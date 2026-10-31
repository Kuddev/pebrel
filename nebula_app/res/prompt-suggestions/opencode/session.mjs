import { createPrediction } from '../shared/client.mjs';

// `prompt` is the official session_prompt slot's TuiPromptRef, never a renderer
// search or an object inferred from textarea properties.
export function connectSession({ api, session, prompt, editable, present, transport }) {
  let revision = 0;
  // The session route can mount after the first submitted prompt starts running.
  let running = (api.state.session.status(session)?.type ?? 'idle') !== 'idle';
  let interacted = false;
  let lastInput;
  let lastParts;
  let lastMode;
  const snapshot = () => {
    const ref = prompt();
    const route = api.route.current;
    return {
      session, revision,
      empty: ref?.current.input === '' && ref.current.parts.length === 0,
      editable: !!ref?.focused && editable() && !api.ui.dialog.open
        && route.name === 'session' && route.params.sessionID === session
        && !api.state.session.get(session)?.parentID && !interacted,
      idle: (api.state.session.status(session)?.type ?? 'idle') === 'idle',
    };
  };
  const prediction = createPrediction({
    agent: 'opencode', transport, snapshot, present,
    insert: text => {
      const ref = prompt();
      ref.set({ ...ref.current, input: text });
    },
  });
  function changed() {
    const current = prompt()?.current;
    if (current?.input !== lastInput || current?.parts.length !== lastParts || current?.mode !== lastMode) {
      lastInput = current?.input;
      lastParts = current?.parts.length;
      lastMode = current?.mode;
      revision++;
      if (running && (lastInput || lastParts)) interacted = true;
      prediction.cancel();
    }
    prediction.validate();
  }
  const right = api.keymap.createKeyMatcher('right');
  const unsubKey = api.keymap.intercept('key', ctx => {
    if (right(ctx.event) && prediction.read().kind !== 'hidden') {
      if (prediction.accept()) ctx.consume();
      // An arrow on an empty loading prompt keeps native navigation semantics.
      return;
    }
    // This only invalidates an existing snapshot; input remains owned by Prompt.
    if (running) interacted = true;
    prediction.cancel();
  });
  const unsubStatus = api.event.on('session.status', event => {
    if (event.properties.sessionID !== session) return;
    if (event.properties.status.type !== 'idle') {
      if (!running) interacted = false;
      running = true;
      prediction.cancel();
      return;
    }
    if (!running) return;
    running = false;
    // The host's native state store settles in the same event dispatch turn.
    queueMicrotask(() => {
      const messages = api.state.session.messages(session);
      const last = messages.findLast(message => message.role === 'assistant');
      if (!last || last.error || last.finish !== 'stop' || !last.time.completed) return;
      const conversation = messages.filter(message => ['user', 'assistant'].includes(message.role))
        .slice(-24).map(message => ({
          role: message.role,
          text: api.state.part(message.id).filter(part => part.type === 'text' && !part.ignored)
            .map(part => part.text).join('\n'),
        }));
      void prediction.completed(last.id, conversation);
    });
  });
  const unsubError = api.event.on('session.error', event => {
    if (event.properties.sessionID !== session) return;
    running = false;
    prediction.cancel();
  });
  return {
    changed,
    read: prediction.read,
    dispose() { unsubKey(); unsubStatus(); unsubError(); prediction.dispose(); },
  };
}
