// Compose the SDK's editor and Text component. Native cursor/IME rendering and
// editing stay untouched; the read-only hint is never part of the draft.
/**
 * @param {typeof import('@earendil-works/pi-coding-agent').CustomEditor} CustomEditor
 * @param {typeof import('@earendil-works/pi-tui').Text} Text
 */
export function predictionEditor(CustomEditor, Text, { prediction, changed, hint, dim }) {
  return class PredictionEditor extends CustomEditor {
    constructor(...args) {
      super(...args);
      this.predictionHint = new Text('', 1, 0);
      this.hintPadding = -1;
      this.hintText = '';
      let hostChange = this.onChange;
      const notify = text => { changed(text); hostChange?.(text); };
      // Pi installs its onChange after construction. Preserve that host callback
      // while observing native programmatic edits, history and bracketed paste.
      Object.defineProperty(this, 'onChange', {
        configurable: true,
        get: () => notify,
        set: callback => { hostChange = callback; },
      });
    }
    handleInput(data) {
      if (this.keybindings.matches(data, 'tui.input.tab')
          && !this.isShowingAutocomplete()) {
        if (prediction().accept()) return;
        // Tab belongs to this pending suggestion. Do not open Pi's file menu
        // while waiting, and never put the loading label into the native draft.
        if (prediction().read().kind === 'loading') return;
      }
      prediction().cancel();
      super.handleInput(data);
    }
    render(width) {
      const native = super.render(width);
      const text = this.focused && this.getText() === '' && !this.isShowingAutocomplete() ? hint() : '';
      if (!text) return native;
      const padding = this.getPaddingX();
      const styled = dim(text);
      if (padding !== this.hintPadding) {
        this.predictionHint.setPaddingX(padding);
        this.hintPadding = padding;
      }
      if (styled !== this.hintText) {
        this.predictionHint.setText(styled);
        this.hintText = styled;
      }
      return native.concat(this.predictionHint.render(width));
    }
  };
}
