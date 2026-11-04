//! Mouse and keyboard gestures stay scoped to the remote list's focus handle.

use super::*;

impl NebulaWorkspace {
    pub(super) fn remote_row_press(
        &mut self,
        row: &SftpEntry,
        event: &gpui::MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        self.remote_browser.last_outcome = None;
        if let Some(focus) = &self.remote_browser.focus_handle {
            focus.focus(window, cx);
        }
        let rows = self.remote_rows();
        let paths: Vec<_> = rows.iter().map(|row| row.path.as_str()).collect();
        if let Some(index) = paths.iter().position(|path| *path == row.path) {
            let modifiers = event.modifiers;
            self.remote_browser.selection.press(
                &paths,
                index,
                modifiers.control || modifiers.platform,
                modifiers.shift,
            );
        }
        cx.notify();
    }

    pub(super) fn remote_files_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self
            .remote_browser
            .focus_handle
            .as_ref()
            .is_some_and(|f| f.contains_focused(window, cx))
        {
            return;
        }
        let modifiers = event.keystroke.modifiers;
        if modifiers.alt {
            return;
        }
        let toggle = modifiers.control || modifiers.platform;
        let rows = self.remote_rows();
        let paths: Vec<_> = rows.iter().map(|row| row.path.as_str()).collect();
        match event.keystroke.key.as_str() {
            "up" | "down" | "home" | "end" => {
                let delta = match event.keystroke.key.as_str() {
                    "up" => -1,
                    "down" => 1,
                    "home" => -(paths.len() as isize),
                    _ => paths.len() as isize,
                };
                self.remote_browser.selection.step(&paths, delta, toggle, modifiers.shift);
                if let Some(index) = paths
                    .iter()
                    .position(|path| Some(*path) == self.remote_browser.selection.focus())
                {
                    self.remote_files_scroll.scroll_to_item(index, gpui::ScrollStrategy::Nearest);
                }
            },
            "a" if toggle => {
                let paths: Vec<_> =
                    rows.iter().filter(|row| !row.is_parent).map(|row| row.path.as_str()).collect();
                self.remote_browser.selection.select_all(&paths);
            },
            "space" if toggle => self.remote_browser.selection.toggle_focus(),
            "c" if toggle => self.remote_copy_selected(window, cx),
            "v" if toggle => self.remote_paste(window, cx),
            "escape" => self.remote_browser.selection.clear(),
            "enter" if !toggle && !modifiers.shift => {
                if let Some(row) = rows
                    .iter()
                    .find(|row| Some(row.path.as_str()) == self.remote_browser.selection.focus())
                    .cloned()
                {
                    self.remote_activate(row, true, window, cx);
                }
            },
            "backspace" if !toggle && !modifiers.shift => self.remote_parent(cx),
            "f5" => self.remote_refresh(cx),
            _ => return,
        }
        cx.stop_propagation();
        self.remote_browser.last_outcome = None;
        if let Some(focus) = &self.remote_browser.focus_handle {
            focus.focus(window, cx);
        }
        cx.notify();
    }

    pub(super) fn remote_action_selection(&mut self, row: &SftpEntry) {
        self.remote_browser.last_outcome = None;
        if !self.remote_browser.selection.contains(&row.path) {
            self.remote_browser.selection.select_only(&row.path);
        }
        self.remote_browser.selection.drag_started();
    }
}
