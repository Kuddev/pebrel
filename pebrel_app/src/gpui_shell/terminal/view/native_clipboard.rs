//! Native clipboard delivery is scoped to the current local PowerShell editor.

use super::{Context, TermMode, TerminalView, keymap};

#[derive(Default)]
pub(super) struct NativeClipboard {
    owner: Option<String>,
}

impl NativeClipboard {
    pub(super) fn clear(&mut self) {
        self.owner = None;
    }

    pub(super) fn observe(&mut self, name: &str, value: &str) {
        if name == "pebrel_shell" {
            // Even the same shell may have changed its bindings since the last prompt.
            self.clear();
        } else if name == "pebrel_clipboard_ready" {
            self.owner = (value.starts_with("pwsh:")
                && value.len() <= 256
                && !value.chars().any(char::is_control))
            .then(|| value.to_owned());
        }
    }
}

impl TerminalView {
    pub(super) fn native_clipboard_ready(&self) -> bool {
        if !crate::platform::CAPABILITIES.native_shell_clipboard
            || self.exited.is_some()
            || self.ssh_destination.is_some()
            || !self.suggest.suggest_env.is_this_machine()
            || self.runtime_agent().is_some()
            || self.command_running
                && !self.command_running_disproved
                && self.native_prompt_epoch != Some(self.prompt_input_epoch)
        {
            return false;
        }
        let Some(owner) = self.native_clipboard.owner.as_deref() else { return false };
        self.suggest.completion_context.owns_current(owner)
            && self.session.as_ref().is_some_and(|session| {
                let term = session.term.lock();
                term.nebula_prompt_active()
                    && !term.mode().intersects(
                        TermMode::ALT_SCREEN
                            | TermMode::VI
                            | TermMode::BRACKETED_PASTE
                            | TermMode::KITTY_KEYBOARD_PROTOCOL,
                    )
            })
    }

    pub(super) fn try_native_clipboard_paste(
        &mut self,
        text: &str,
        emit: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        self.sync_native_prompt();
        if text.is_empty() || !self.native_clipboard_ready() {
            return false;
        }
        // Only delegate a current clipboard action. Frozen dialog text, dropped
        // paths and programmatic payloads must not silently read a different clip.
        if cx.read_from_clipboard().and_then(|item| item.text()).as_deref() != Some(text) {
            return false;
        }
        let key = gpui::Keystroke::parse("ctrl-shift-f11").unwrap();
        let Some(bytes) = keymap::encode(&key, &self.term_mode()) else { return false };
        crate::display::nebula_input_text(&mut self.suggest, text);
        if emit {
            self.write_user_text(text.to_owned(), true, bytes, cx);
        } else {
            self.write_input(bytes, cx);
        }
        true
    }
}
