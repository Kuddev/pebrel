/** @jsxImportSource @opentui/solid */
import type { TuiPlugin, TuiPromptRef, TuiHostSlotMap } from '@opencode-ai/plugin/tui';
import { createEffect, createSignal, onCleanup, Show } from 'solid-js';
import { createTransport } from '../shared/client.mjs';
import { connectSession } from './session.mjs';

export const id = 'pebrel-prompt-suggestions';

// OpenCode 1.19.5's public TUI API. V2 uses a different plugin contract and must
// not be treated as compatible merely because it also renders a textarea.
export const tui: TuiPlugin = async api => {
  const version = /^1\.19\.(\d+)(?:$|[-+])/u.exec(api.app.version);
  if (!version || Number(version[1]) < 5) return;
  const transport = createTransport();
  if (!transport) return;
  const SessionPrompt = (props: TuiHostSlotMap['session_prompt']) => {
    const [prompt, setPrompt] = createSignal<TuiPromptRef>();
    const [hint, setHint] = createSignal('');
    const adapter = connectSession({
      api, session: props.session_id, prompt, transport,
      editable: () => props.visible !== false && !props.disabled,
      present: (state: { text: string }) => setHint(state.text),
    });
    createEffect(() => {
      // Subscribe to the native draft, permission state and route. No polling,
      // file reads, terminal-output parsing or duplicate editing buffer.
      prompt()?.current.input;
      prompt()?.current.parts.length;
      prompt()?.current.mode;
      props.visible;
      props.disabled;
      api.ui.dialog.open;
      api.route.current;
      adapter.changed();
    });
    onCleanup(() => adapter.dispose());
    const removeDispose = api.lifecycle.onDispose(() => adapter.dispose());
    onCleanup(removeDispose);
    return <api.ui.Prompt
      sessionID={props.session_id}
      visible={props.visible}
      disabled={props.disabled}
      onSubmit={props.on_submit}
      ref={ref => { setPrompt(ref); props.ref?.(ref); }}
      hint={hint() ? <text fg={api.theme.current.textMuted}>{hint()}</text> : undefined}
      right={<api.ui.Slot name="session_prompt_right" session_id={props.session_id} />}
    />;
  };
  api.slots.register({ slots: {
    session_prompt: (_context, props) => <Show when={props.session_id} keyed>
      {session => <SessionPrompt {...props} session_id={session} />}
    </Show>,
  } });
};

export default { id, tui };
