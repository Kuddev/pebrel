//! Pointer edits operate on a verified native shell snapshot; TUI input is untouched.
mod layout;
#[cfg(test)]
mod tests;

use std::time::{Duration, Instant};

use super::*;

#[derive(Clone, Copy, PartialEq)]
struct Position(TermPoint, Side);

struct Request {
    nonce: usize,
    epoch: u64,
    created: Instant,
    start: Position,
    end: Position,
    origin: Point<Pixels>,
    dimensions: (usize, usize),
    delete_requested: bool,
    screen: layout::Screen,
}

struct Selected {
    start: usize,
    end: usize,
    screen: layout::Screen,
    input_end: TermPoint,
    highlight: Selection,
}

#[derive(Default)]
pub(super) struct Pointer {
    owner: Option<String>,
    ready: bool,
    query_count: usize,
    press: Option<Position>,
    pending: Option<Request>,
    selected: Option<Selected>,
}

impl Pointer {
    pub(super) fn revoke(&mut self) {
        self.ready = false;
        self.invalidate();
    }

    pub(super) fn invalidate(&mut self) {
        self.press = None;
        self.selected = None;
        self.pending = None;
    }
}

fn command(argument: usize) -> Vec<u8> {
    debug_assert!(argument <= 999999);
    let mut bytes = Vec::with_capacity(16);
    for digit in argument.to_string().bytes() {
        bytes.extend_from_slice(&[0x1b, digit]);
    }
    bytes.extend_from_slice(b"\x1b[45~");
    bytes
}

impl TerminalView {
    fn shell_pointer_command(&self, argument: usize) -> Vec<u8> {
        if !self.shell_pointer.owner.as_deref().is_some_and(|owner| owner.starts_with("pwsh:")) {
            return command(argument);
        }
        let mode = self.term_mode();
        let prefix =
            super::super::keymap::encode(&gpui::Keystroke::parse("ctrl-shift-f10").unwrap(), &mode)
                .unwrap();
        let query =
            super::super::keymap::encode(&gpui::Keystroke::parse("ctrl-shift-f12").unwrap(), &mode)
                .unwrap();
        let mut bytes = Vec::with_capacity(128);
        for digit in argument.to_string().chars() {
            bytes.extend_from_slice(&prefix);
            bytes.extend(super::super::keymap::encode_choice_text(&digit.to_string(), &mode));
        }
        bytes.extend(query);
        bytes
    }

    pub(super) fn shell_pointer_preserves_native_undo(&self, bytes: &[u8]) -> bool {
        // PSReadLine uses Ctrl+X as its Emacs undo prefix and Ctrl+Z in Windows
        // mode. Elsewhere these can suspend a process or end input; scope this
        // exception to the current advertised native editor, never a TUI.
        self.shell_pointer.owner.as_deref().is_some_and(|owner| owner.starts_with("pwsh:"))
            && self.shell_pointer_allowed()
            && ["ctrl-x", "ctrl-z"].iter().any(|key| {
                super::super::keymap::encode(
                    &gpui::Keystroke::parse(key).unwrap(),
                    &self.term_mode(),
                )
                .is_some_and(|encoded| encoded == bytes)
            })
    }

    fn shell_pointer_allowed(&self) -> bool {
        self.exited.is_none()
            && self.shell_pointer.ready
            && self.runtime_chat_agent().is_none()
            && !(self.command_running
                && !self.command_running_disproved
                && self.native_prompt_epoch != Some(self.prompt_input_epoch))
            && self.shell_pointer.owner.as_deref().is_some_and(|owner| {
                self.suggest.completion_context.owns_current(owner)
                    && (owner.starts_with("bash:")
                        || owner.starts_with("zsh:")
                        || owner.starts_with("pwsh:")
                        || owner.starts_with("wsl|"))
            })
            && self.session.as_ref().is_some_and(|session| {
                let term = session.term.lock();
                term.nebula_prompt_active()
                    && term.nebula_prompt_input_point().is_some()
                    && term.grid().display_offset() == 0
                    && !term
                        .mode()
                        .intersects(TermMode::ALT_SCREEN | TermMode::MOUSE_MODE | TermMode::VI)
            })
    }

    pub(super) fn begin_shell_pointer(&mut self, event: &MouseDownEvent) {
        self.shell_pointer.invalidate();
        self.sync_native_prompt();
        if event.button != MouseButton::Left
            || event.click_count != 1
            || event.modifiers != gpui::Modifiers::default()
            || !self.shell_pointer_allowed()
        {
            return;
        }
        let (point, side) = self.grid_point(event.position);
        let Some(input) =
            self.session.as_ref().and_then(|s| s.term.lock().nebula_prompt_input_point())
        else {
            return;
        };
        if point >= input {
            self.shell_pointer.press = Some(Position(point, side));
        }
    }

    pub(super) fn finish_shell_pointer(&mut self, event: &MouseUpEvent) {
        let Some(start) = self.shell_pointer.press.take() else { return };
        if event.button != MouseButton::Left
            || event.modifiers != gpui::Modifiers::default()
            || !self.shell_pointer_allowed()
            || self.shell_pointer.query_count >= 980000
        {
            return;
        }
        let (point, side) = self.grid_point(event.position);
        let nonce = 20000 + self.shell_pointer.query_count;
        if let Some(session) = &self.session
            && let Some(selection) = session.term.lock().selection.as_mut()
        {
            // The final move may be coalesced; the displayed range and native
            // edit must both use the release point, even when ZLE never clears it.
            selection.update(point, side);
        }
        let Some(screen) =
            self.session.as_ref().and_then(|session| layout::Screen::capture(&session.term.lock()))
        else {
            return;
        };
        self.shell_pointer.query_count += 1;
        self.shell_pointer.pending = Some(Request {
            nonce,
            epoch: self.prompt_input_epoch,
            created: Instant::now(),
            start,
            end: Position(point, side),
            origin: self.origin,
            dimensions: (self.cols, self.rows),
            delete_requested: false,
            screen,
        });
        self.write_bytes(self.shell_pointer_command(nonce));
    }

    pub(super) fn observe_shell_pointer(
        &mut self,
        name: &str,
        value: &str,
        cx: &mut Context<Self>,
    ) {
        if name == "pebrel_shell" {
            self.shell_pointer.revoke();
            return;
        }
        if name == "pebrel_pointer_ready" {
            if self.suggest.completion_context.owns_current(value) {
                self.shell_pointer.invalidate();
                self.shell_pointer.owner = Some(value.to_owned());
                self.shell_pointer.ready = true;
            }
            return;
        }
        if name == "pebrel_pointer_status" {
            if value.split_once('\n').is_some_and(|(owner, status)| {
                self.shell_pointer.owner.as_deref() == Some(owner) && status == "rejected"
            }) {
                self.shell_pointer.invalidate();
            }
            return;
        }
        if name != "pebrel_pointer" {
            return;
        }
        let mut fields = value.splitn(4, '\n');
        if fields.next() != self.shell_pointer.owner.as_deref() {
            return;
        }
        let Some((unit, nonce)) = fields.next().and_then(|unit| unit.split_once(':')) else {
            return;
        };
        let Ok(nonce) = nonce.parse::<usize>() else { return };
        if !self.shell_pointer.pending.as_ref().is_some_and(|request| request.nonce == nonce) {
            return;
        }
        let request = self.shell_pointer.pending.take().unwrap();
        if request.epoch != self.prompt_input_epoch
            || request.created.elapsed() > Duration::from_secs(1)
            || request.origin != self.origin
            || request.dimensions != (self.cols, self.rows)
            || !self.shell_pointer_allowed()
            || value.len() > 4352
        {
            return;
        }
        if unit != "utf8" {
            return;
        }
        let Some(cursor) = fields.next().and_then(|cursor| cursor.parse::<usize>().ok()) else {
            return;
        };
        let Some(line) = fields.next() else { return };
        let Some(layout) = layout::Layout::read(&request.screen, line, cursor) else { return };
        let Some(start) = layout.offset(request.start.0, request.start.1) else { return };
        let Some(end) = layout.offset(request.end.0, request.end.1) else { return };
        self.shell_pointer.selected = (start != end).then(|| {
            let (start_point, start_side) = layout.position(start).unwrap();
            let (end_point, end_side) = layout.position(end).unwrap();
            let mut highlight = Selection::new(SelectionType::Simple, start_point, start_side);
            highlight.update(end_point, end_side);
            Selected { start, end, screen: request.screen, input_end: layout.end, highlight }
        });
        self.completion_editor.invalidate();
        self.editor_query_task = None;
        self.suggest.clear_completion_hints();
        self.write_bytes(self.shell_pointer_command(1 + end));
        self.restore_shell_pointer_selection();
        if request.delete_requested {
            self.delete_shell_pointer_selection("delete", cx);
        }
        cx.notify();
    }

    pub(super) fn delete_shell_pointer_selection(
        &mut self,
        key: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        if !matches!(key, "backspace" | "delete") || !self.shell_pointer_allowed() {
            return false;
        }
        let Some(selected) = self.shell_pointer.selected.take() else {
            // A fast Delete after drag must not delete at the old native caret
            // while the query is in flight. A rejected/expired query is a no-op.
            if let Some(request) = self.shell_pointer.pending.as_mut()
                && request.start != request.end
            {
                if request.created.elapsed() > Duration::from_secs(1) {
                    self.shell_pointer.pending = None;
                    if let Some(session) = &self.session {
                        session.term.lock().selection = None;
                    }
                    cx.notify();
                    return true;
                }
                request.delete_requested = true;
                return true;
            }
            return false;
        };
        let mut bytes = self.shell_pointer_command(4098 + selected.start);
        bytes.extend(self.shell_pointer_command(8195 + selected.end));
        // The editor checks the unchanged native buffer/cursor again for both
        // callbacks. Consume this key even on rejection; never delete elsewhere.
        self.write_editor_input(bytes, true, cx);
        true
    }

    pub(super) fn restore_shell_pointer_selection(&mut self) {
        let Some(selected) = self.shell_pointer.selected.as_ref() else { return };
        if !self.shell_pointer_allowed() {
            return;
        }
        let Some(session) = &self.session else { return };
        let mut term = session.term.lock();
        // bind-x clears the terminal's copy selection while redisplaying. Only
        // reapply the verified input range after those same cells are back.
        if selected.screen.matches(&term, selected.input_end) {
            term.selection = Some(selected.highlight.clone());
        }
    }
}
