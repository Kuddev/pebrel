import test from 'node:test';
import assert from 'node:assert/strict';
import { pathToFileURL } from 'node:url';
import { predictionEditor } from './editor.mjs';
import { createPrediction } from '../shared/client.mjs';

// Point at an external, pinned SDK install. No upstream source is vendored.
const root = process.env.PEBREL_PI_NATIVE_QA_MODULES;
const native = root ? await import(pathToFileURL(`${root}/@earendil-works/pi-tui/dist/index.js`)) : undefined;
const agent = root ? await import(pathToFileURL(
  `${root}/@earendil-works/pi-coding-agent/dist/modes/interactive/components/custom-editor.js`)) : undefined;
const flush = () => new Promise(resolve => setImmediate(resolve));

test('official Pi CustomEditor retains Unicode input/undo/host callbacks and native render',
  { skip: !root && 'set PEBREL_PI_NATIVE_QA_MODULES to the Pi 1.4.0 SDK node_modules' }, async () => {
    let editor;
    let revision = 0;
    let resolve;
    let submits = 0;
    const changes = [];
    const tui = { requestRender() {}, terminal: { columns: 80, rows: 24 } };
    const theme = { borderColor: text => text, selectList: {} };
    const prediction = createPrediction({ agent: 'pi',
      snapshot: () => ({ session: 'pi-native', revision, empty: editor.getText() === '',
        editable: editor.focused, idle: true }),
      transport: { config: async () => ({ enabled: true, loading_text: 'sol 正在预测' }),
        predict: () => new Promise(done => { resolve = done; }) },
      present: () => {}, insert: text => editor.insertTextAtCursor(text),
    });
    const Editor = predictionEditor(agent.CustomEditor, native.Text, {
      prediction: () => prediction,
      changed: () => { revision++; prediction.cancel(); },
      hint: () => prediction.read().text,
      dim: text => `\x1b[2m${text}\x1b[22m`,
    });
    editor = new Editor(tui, theme, native.getKeybindings());
    editor.focused = true;
    // Match Pi's host: callbacks are assigned after construction.
    editor.onChange = text => changes.push(text);
    editor.onSubmit = () => { submits++; };
    editor.handleInput('猫');
    editor.handleInput('😺');
    assert.equal(editor.getText(), '猫😺');
    assert.equal(changes.at(-1), '猫😺');
    editor.setText('');
    let nativeCompletions = 0;
    editor.setAutocompleteProvider({ getSuggestions: async () => { nativeCompletions++; return null; } });
    const plain = agent.CustomEditor.prototype.render.call(editor, 80);
    const work = prediction.completed('native-turn-1', []);
    await flush();
    const loading = editor.render(80);
    assert.deepEqual(loading.slice(0, plain.length), plain);
    assert.match(loading.slice(plain.length).join(''), /sol 正在预测\./u);
    assert.equal(editor.getText(), '');
    assert.equal(prediction.accept(), false);
    editor.handleInput('\t');
    await flush();
    assert.equal(nativeCompletions, 0);
    assert.equal(prediction.read().kind, 'loading');
    resolve({ suggestion: '再检查一下喵😺' });
    await work;
    editor.handleInput('\t');
    assert.equal(editor.getText(), '再检查一下喵😺');
    assert.equal(submits, 0);
    // SDK undo is one atomic native insert, not synthesized keypresses.
    editor.handleInput('\x1f');
    assert.equal(editor.getText(), '');
    editor.handleInput('\t');
    await flush();
    assert.equal(nativeCompletions, 1);
    const paste = prediction.completed('native-turn-2', []);
    await flush();
    editor.handleInput('\x1b[200~手动粘贴中文\x1b[201~');
    resolve({ suggestion: '迟到的预测' });
    await paste;
    assert.equal(editor.getText(), '手动粘贴中文');
    assert.equal(prediction.read().kind, 'hidden');
    prediction.dispose();
  });
