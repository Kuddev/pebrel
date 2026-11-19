import { CustomEditor, VERSION, type ExtensionAPI, type ExtensionContext } from '@earendil-works/pi-coding-agent';
import { Text } from '@earendil-works/pi-tui';
import { createPrediction, createTransport, textExcerpt } from '../shared/client.mjs';
import { predictionEditor } from './editor.mjs';

// Verified against Pi 1.4.0's public extension API. Older/forked SDKs must not
// silently fall back to terminal-output or raw-key input reconstruction.
export default function (pi: ExtensionAPI) {
  const version = /^1\.(\d+)\.\d+(?:$|[-+])/u.exec(VERSION);
  if (!version || Number(version[1]) < 4) return;
  const transport = createTransport();
  if (!transport) return;
  let context: ExtensionContext | undefined;
  let editor: CustomEditor | undefined;
  let factory: ReturnType<ExtensionContext['ui']['getEditorComponent']>;
  let revision = 0;
  let draft = '';
  let blocked = false;
  let success = false;
  let redraw = () => {};
  const prediction = createPrediction({
    agent: 'pi', transport,
    snapshot: () => ({
      session: context?.sessionManager.getSessionId(), revision,
      empty: editor?.getText() === '',
      editable: !!editor?.focused && !blocked && !editor.isShowingAutocomplete()
        && context?.ui.getEditorComponent() === factory,
      idle: context?.isIdle() === true,
    }),
    present: () => redraw(),
    insert: (text: string) => editor?.insertTextAtCursor(text),
  });
  const detach = () => {
    prediction.cancel();
    if (factory && context?.ui.getEditorComponent() === factory) context.ui.setEditorComponent(undefined);
    editor = undefined;
    factory = undefined;
    redraw = () => {};
  };
  pi.on('session_start', (_event, ctx) => {
    detach();
    context = ctx;
    revision++;
    success = false;
    blocked = false;
    // Respect an editor installed by another extension.
    if (ctx.mode !== 'tui' || ctx.ui.getEditorComponent()) return;
    const Editor = predictionEditor(CustomEditor, Text, {
      prediction: () => prediction,
      changed: (text: string) => {
        if (text !== draft) { draft = text; revision++; prediction.cancel(); }
      },
      hint: () => prediction.read().text,
      dim: (text: string) => ctx.ui.theme.fg('dim', text),
    });
    factory = (tui, theme, keybindings) => {
      redraw = () => tui.requestRender();
      editor = new Editor(tui, theme, keybindings);
      return editor!;
    };
    ctx.ui.setEditorComponent(factory);
  });
  const changingSession = () => { prediction.cancel(); revision++; success = false; };
  pi.on('session_before_switch', changingSession);
  pi.on('session_before_fork', changingSession);
  pi.on('session_before_tree', changingSession);
  pi.on('session_shutdown', detach);
  pi.on('agent_start', () => { success = false; prediction.cancel(); });
  pi.on('agent_end', event => {
    const last = event.messages.findLast(message => message.role === 'assistant');
    success = last?.role === 'assistant' && last.stopReason === 'stop';
  });
  pi.on('ui_prompt_start', () => { blocked = true; prediction.cancel(); });
  pi.on('ui_prompt_end', () => { blocked = false; });
  pi.on('agent_settled', (event, ctx) => {
    if (event.aborted || !success || !editor) return;
    context = ctx;
    // Do not await the network in Pi's event dispatch path.
    void prediction.completed(ctx.sessionManager.getLeafId(), ctx.sessionManager.getBranch(), {
      roleOf: entry => entry.type === 'message' ? entry.message.role : undefined,
      textOf: (entry, limit) => {
        const content = entry.message.content;
        return typeof content === 'string' ? textExcerpt([content], undefined, limit)
          : textExcerpt(content, block => block.type === 'text' ? block.text : undefined, limit);
      },
    });
  });
}
