//! GPUI lifetime adapter for the testable, presentation-only trajectory.

use super::super::cursor_motion::{CursorMotion, Position};
use super::*;
use nebula_terminal::render::CursorSnapshot;
use nebula_terminal::vte::ansi::CursorShape;
use std::time::{Duration, Instant};

// PSReadLine hides the host cursor while repainting each input echo. Preserve
// its last visible trajectory across that brief gap, but not a long hide.
const REPAINT_HIDE_GRACE: Duration = Duration::from_millis(250);

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests;

#[cfg(all(test, feature = "gpui-test-support"))]
mod native_tests;

#[cfg(all(test, windows, feature = "gpui-test-support"))]
mod typing_tests;

#[derive(Clone, Copy, PartialEq)]
pub(in crate::gpui_shell::terminal) struct CursorGeometry {
    pub screen: (usize, bool, i64),
    pub grid: (usize, usize),
    pub metrics: (f32, f32, f32),
    pub projection_shift: i32,
    pub shape: CursorShape,
}

pub(super) struct CursorAnimation {
    clock: Instant,
    motion: CursorMotion,
    geometry: Option<CursorGeometry>,
    hidden_since: Option<Duration>,
    frame_pending: bool,
}

impl Default for CursorAnimation {
    fn default() -> Self {
        Self {
            clock: Instant::now(),
            motion: Default::default(),
            geometry: None,
            hidden_since: None,
            frame_pending: false,
        }
    }
}

impl CursorAnimation {
    pub fn reset(&mut self) {
        self.motion.reset();
        self.geometry = None;
        self.hidden_since = None;
    }
    pub fn note_input(&mut self, bytes: &[u8]) {
        self.motion.note_input(bytes, self.clock.elapsed());
    }

    pub fn note_encoded_key(&mut self, key: &gpui::Keystroke, encoded: &[u8]) {
        // Input intent must survive negotiated extended key encodings. Reuse
        // the existing encoder for classification only; never change PTY bytes.
        if let Some(plain) = keymap::encode(key, &TermMode::empty()) {
            if plain != encoded {
                self.note_input(&plain);
            }
        } else if !key.modifiers.control && !key.modifiers.alt && !key.modifiers.platform {
            if let Some(text) = &key.key_char {
                self.note_input(text.as_bytes());
            }
        }
    }
}

impl TerminalView {
    pub(in crate::gpui_shell::terminal) fn visual_cursor(
        &mut self,
        cursor: Option<&CursorSnapshot>,
        visual_col: usize,
        geometry: CursorGeometry,
        at_bottom: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Position> {
        let enabled = cx.try_global::<Settings>().is_some_and(|settings| {
            settings.cursor_motion == nebula_settings::CursorMotion::Smooth
        }) && self.output_visible
            && at_bottom
            && !cx.reduce_motion()
            && self.marked_text.as_deref().is_none_or(str::is_empty);
        let now = self.cursor_animation.clock.elapsed();
        let Some(cursor) = cursor.filter(|cursor| cursor.shape != CursorShape::Hidden) else {
            let same_surface = self.cursor_animation.geometry.is_some_and(|previous| {
                previous.screen == geometry.screen
                    && previous.grid == geometry.grid
                    && previous.metrics == geometry.metrics
            });
            if enabled && same_surface {
                let since = *self.cursor_animation.hidden_since.get_or_insert(now);
                if now.saturating_sub(since) >= REPAINT_HIDE_GRACE {
                    self.cursor_animation.reset();
                }
            } else {
                self.cursor_animation.reset();
            }
            // Never paint or request frames for a VT-hidden cursor. Hidden
            // repaint coordinates are not a new target or input authorization.
            return None;
        };
        let expired = self
            .cursor_animation
            .hidden_since
            .take()
            .is_some_and(|since| now.saturating_sub(since) >= REPAINT_HIDE_GRACE);
        if expired || self.cursor_animation.geometry != Some(geometry) {
            self.cursor_animation.reset();
            self.cursor_animation.geometry = Some(geometry);
        }
        let position = self.cursor_animation.motion.update(
            Position::new(visual_col as f64, cursor.row as f64),
            now,
            enabled,
            !geometry.screen.1,
            geometry.grid.0,
        );
        if self.cursor_animation.motion.active() && enabled && !self.cursor_animation.frame_pending
        {
            self.cursor_animation.frame_pending = true;
            // The callback belongs to this Entity, not the PTY or a global timer.
            cx.on_next_frame(window, |view, _, cx| {
                view.cursor_animation.frame_pending = false;
                if view.output_visible
                    && view.cursor_animation.hidden_since.is_none()
                    && view.cursor_animation.motion.active()
                {
                    cx.notify();
                }
            });
        }
        Some(position)
    }
}
