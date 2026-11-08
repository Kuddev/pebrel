//! A left-button press owns its motion/release, even outside the pane's hitbox.
//! Text ranges and edits remain owned by the application reading the PTY.
use super::*;
use nebula_terminal::Term;
use nebula_terminal::sync::FairMutex;
use std::sync::Weak;

#[cfg(all(test, feature = "gpui-test-support"))]
mod native_tests;

pub(super) struct Gesture {
    terminal: Weak<FairMutex<Term<session::EventProxy>>>,
    generation: u64,
    point: TermPoint,
    modifiers: gpui::Modifiers,
}

impl TerminalView {
    pub(super) fn begin_application_mouse(
        &mut self,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.session.as_ref() else { return };
        let (point, _) = self.grid_point(event.position);
        let mut term = session.term.lock();
        if !term.mode().intersects(TermMode::MOUSE_MODE) || self.exited.is_some() {
            return;
        }
        let Some(bytes) = mouse_protocol::report(
            term.mode(),
            point,
            mouse_protocol::BUTTON_LEFT,
            true,
            report_mods(event.modifiers),
        ) else {
            return;
        };
        self.application_mouse = Some(Gesture {
            terminal: Arc::downgrade(&session.term),
            generation: term.mouse_input_generation(),
            point,
            modifiers: event.modifiers,
        });
        term.selection = None;
        drop(term);
        self.selecting = false;
        self.stop_selection_scroll();
        self.last_report_point = Some(point);
        self.write_bytes(bytes);
        cx.notify();
    }

    /// Capture listeners call this for every pane. Only the press owner consumes it.
    pub(in crate::gpui_shell::terminal) fn move_application_mouse(
        &mut self,
        event: &MouseMoveEvent,
    ) -> bool {
        let Some(gesture) = self.application_mouse.as_ref() else { return false };
        if event.pressed_button != Some(MouseButton::Left) {
            // Recover from a missed OS release without moving the selection to a
            // new hover position. Never synthesize a new press on re-entry.
            self.finish_application_mouse(None);
            return false;
        }
        let (point, _) = self.grid_point(event.position);
        if point != gesture.point || event.modifiers != gesture.modifiers {
            self.report_application_mouse(point, event.modifiers, true);
        }
        true
    }

    pub(in crate::gpui_shell::terminal) fn finish_application_mouse(
        &mut self,
        event: Option<&MouseUpEvent>,
    ) -> bool {
        let Some(gesture) = self.application_mouse.as_ref() else { return false };
        let (point, modifiers) = match event {
            Some(event) => (self.grid_point(event.position).0, event.modifiers),
            None => (gesture.point, gesture.modifiers),
        };
        self.report_application_mouse(point, modifiers, false);
        self.application_mouse = None;
        self.last_report_point = None;
        true
    }

    fn report_application_mouse(
        &mut self,
        point: TermPoint,
        modifiers: gpui::Modifiers,
        drag: bool,
    ) {
        let Some(gesture) = self.application_mouse.as_ref() else { return };
        let Some(session) = self.session.as_ref() else {
            self.application_mouse = None;
            return;
        };
        let term = session.term.lock();
        if !std::ptr::eq(gesture.terminal.as_ptr(), Arc::as_ptr(&session.term))
            || gesture.generation != term.mouse_input_generation()
            || !term.mode().intersects(TermMode::MOUSE_MODE)
            || self.exited.is_some()
        {
            self.application_mouse = None;
            return;
        }
        if drag && !term.mode().intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION) {
            return;
        }
        let encode = |point| {
            mouse_protocol::report(
                term.mode(),
                point,
                mouse_protocol::BUTTON_LEFT + if drag { mouse_protocol::DRAG_OFFSET } else { 0 },
                drag,
                report_mods(modifiers),
            )
        };
        let (point, bytes) = match encode(point) {
            Some(bytes) => (point, bytes),
            None if !drag => {
                // Scrollback and legacy coordinate limits cannot encode this
                // position. The app still needs a release for its valid press.
                let Some(bytes) = encode(gesture.point) else { return };
                (gesture.point, bytes)
            },
            None => return,
        };
        drop(term);
        if let Some(gesture) = self.application_mouse.as_mut() {
            gesture.point = point;
            gesture.modifiers = modifiers;
        }
        self.last_report_point = Some(point);
        self.write_bytes(bytes);
    }

    pub(super) fn cancel_pointer_gesture(&mut self) {
        self.finish_application_mouse(None);
        self.selecting = false;
        self.stop_selection_scroll();
        self.scrollbar_drag = None;
        self.completion_viewport.scrollbar_grab = None;
        self.pending_link_open = false;
    }
}

fn report_mods(modifiers: gpui::Modifiers) -> mouse_protocol::ReportMods {
    mouse_protocol::ReportMods {
        shift: modifiers.shift,
        alt: modifiers.alt,
        control: modifiers.control,
    }
}
