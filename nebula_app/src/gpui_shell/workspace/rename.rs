//! Inline title editing; pane identity is independent of tab position and shell titles.
use super::*;

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests;

pub(super) fn normalized_name(buffer: &str) -> Option<String> {
    let trimmed = buffer.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// 侧栏行内重命名的活动状态（旧壳 `nebula_tab_rename` 同形态：被编辑的那
/// 一行原地变输入框，而不是弹一个对话框）。Enter 提交；Esc / 失焦取消
/// （对照 `input/chrome.rs` 点在框外 = `CancelRename`）；提交空串 = 恢复
/// 自动标签名。
pub(super) struct TabRename {
    pub(super) ix: usize,
    pub(super) input: Entity<InputState>,
    _subscription: Subscription,
}

/// 旧壳 `TabRequest::CommitRename`（`window_context.rs` ~871-880）：
/// trim；空串 → `custom_name = None`（恢复自动名）；非空 → `Some(trimmed)`。
pub(super) fn apply_commit_rename(meta: &mut TabMeta, buffer: &str) {
    meta.custom_name = normalized_name(buffer);
}

/// 旧壳 `TabRequest::CancelRename`（`window_context.rs` ~896-901）：
/// 丢掉重命名缓冲，`custom_name` 保持进入编辑前的值。
pub(super) fn apply_cancel_rename(_meta: &mut TabMeta) {}

pub(super) struct PaneRename {
    pub(super) pane_id: u64,
    pub(super) input: Entity<InputState>,
    saved_names: Vec<Option<String>>,
    saved_name_index: usize,
    /// Once typing starts, the input owns text Undo/Redo for the rest of this edit.
    local_edits: bool,
    _subscription: Subscription,
}

impl NebulaWorkspace {
    /// 进入行内重命名：对照旧壳 `TabRequest::BeginRename`
    /// （`window_context.rs` ~854-868）。预填 `custom_name`，否则
    /// `chrome_tab_label`（cwd 末级，不含分屏后缀）。已在编辑别的行时丢掉
    /// 前一次缓冲（不提交），与旧壳覆盖 `nebula_tab_rename` 同合同。
    pub(super) fn begin_rename(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() {
            return;
        }
        // 覆盖前一次编辑：只丢缓冲，不走 Commit（否则会把当时显示的目录名
        // 冻成 custom_name）。不要调用 `cancel_rename`——它会 `focus_active`
        // 把焦点延迟抢回终端，紧接着的输入框 focus 会被下一帧冲掉。
        let _ = self.tab_rename.take();
        self.commit_pane_rename(false, window, cx);
        let current = self.meta(ix).custom_name.unwrap_or_else(|| self.rename_prefill(ix, cx));
        let input = cx.new(|cx| InputState::new(window, cx));
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this: &mut Self, input: &Entity<InputState>, event: &InputEvent, window, cx| {
                if this.tab_rename.as_ref().is_none_or(|edit| edit.input != *input) {
                    return;
                }
                match event {
                    InputEvent::PressEnter { .. } => this.commit_rename(window, cx),
                    // 点在框外 = CancelRename（`input/chrome.rs` ~359-368），
                    // 不是 Commit。Blur 提交会把自动目录名冻成 custom_name。
                    InputEvent::Blur => this.cancel_rename(window, cx),
                    _ => {},
                }
            },
        );
        // 旧壳 `nebula_tab_rename_select_all = true`：set_value 后全选再 focus。
        // `InputState::select_all` 是 `pub(super)`，对外走公开的 `SelectAll` action。
        input.update(cx, |state, cx| {
            state.set_value(current, window, cx);
            state.focus(window, cx);
        });
        self.tab_rename = Some(TabRename { ix, input, _subscription: subscription });
        cx.on_next_frame(window, |this, window, cx| {
            let Some(rename) = this.tab_rename.as_ref() else { return };
            if !rename.input.read(cx).focus_handle(cx).is_focused(window) {
                return;
            }
            window.dispatch_action(Box::new(gpui_component::input::SelectAll), cx);
        });
        cx.notify();
    }

    /// BeginRename 预填：有 custom 用 custom，否则终端用聚焦 pane 的
    /// `tab_label()`（cwd 末级，对齐 `chrome_tab_label`），其它 tab 用标题。
    pub(super) fn rename_prefill(&self, ix: usize, cx: &App) -> String {
        match self.tabs.get(ix) {
            Some(tab @ WorkspaceTab::Terminal { .. }) => tab
                .focused_view()
                .map(|view| view.read(cx).tab_label())
                .unwrap_or_else(|| self.tab_title(ix, cx).to_string()),
            _ => self.tab_title(ix, cx).to_string(),
        }
    }

    pub(super) fn commit_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(rename) = self.tab_rename.take() else { return };
        let name = rename.input.read(cx).value();
        if let Some(meta) = self.tab_meta.get_mut(rename.ix) {
            apply_commit_rename(meta, &name);
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    pub(super) fn cancel_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(rename) = self.tab_rename.take() else { return };
        if let Some(meta) = self.tab_meta.get_mut(rename.ix) {
            apply_cancel_rename(meta);
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    pub(super) fn begin_pane_rename(
        &mut self,
        pane_id: u64,
        automatic_title: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_pane_rename(false, window, cx);
        let Some(ix) = self.tab_of_pane(pane_id) else { return };
        let Some(WorkspaceTab::Terminal { panes, .. }) = self.tabs.get(ix) else { return };
        let Some(pane) = panes.iter().find(|pane| pane.id == pane_id) else { return };
        let current = pane.custom_name.clone().unwrap_or_default();
        let mut saved_names = pane.name_history.clone();
        saved_names.push(pane.custom_name.clone());
        let saved_name_index = saved_names.len() - 1;
        // Drop subscriptions before changing focus, so an old Blur cannot cancel the new editor.
        self.tab_rename = None;
        self.pane_rename = None;
        self.pane_drag = None;
        // An empty value means automatic naming; show the live title as a placeholder.
        // Merely opening and leaving the editor must not freeze a directory/program name.
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(automatic_title));
        input.update(cx, |state, cx| state.set_value(current, window, cx));
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this: &mut Self, input, event: &InputEvent, window, cx| {
                if this.pane_rename.as_ref().is_none_or(|edit| edit.input != *input) {
                    return;
                }
                match event {
                    InputEvent::PressEnter { .. } => this.commit_pane_rename(true, window, cx),
                    // Clicking another pane/control must keep its newly acquired focus.
                    InputEvent::Blur => this.commit_pane_rename(false, window, cx),
                    InputEvent::Change => {
                        if let Some(edit) = this.pane_rename.as_mut() {
                            edit.local_edits = true;
                        }
                    },
                    _ => {},
                }
            },
        );
        self.pane_rename = Some(PaneRename {
            pane_id,
            input: input.clone(),
            saved_names,
            saved_name_index,
            local_edits: false,
            _subscription: subscription,
        });
        cx.on_next_frame(window, move |this, window, cx| {
            if this.pane_rename.as_ref().is_none_or(|edit| edit.input != input) {
                return;
            }
            input.update(cx, |state, cx| state.focus(window, cx));
            window.dispatch_action(Box::new(gpui_component::input::SelectAll), cx);
        });
        cx.notify();
    }

    pub(super) fn commit_pane_rename(
        &mut self,
        restore_focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(edit) = self.pane_rename.take() else { return };
        let name = normalized_name(&edit.input.read(cx).value());
        let editor_focus = edit.input.read(cx).focus_handle(cx);
        if let Some(ix) = self.tab_of_pane(edit.pane_id)
            && let WorkspaceTab::Terminal { panes, .. } = &mut self.tabs[ix]
            && let Some(pane) = panes.iter_mut().find(|pane| pane.id == edit.pane_id)
        {
            if pane.custom_name != name {
                const SAVED_NAME_LIMIT: usize = 32;
                if pane.name_history.len() == SAVED_NAME_LIMIT {
                    pane.name_history.remove(0);
                }
                pane.name_history.push(pane.custom_name.take());
                pane.custom_name = name;
            }
        }
        if restore_focus {
            self.focus_active(window, cx);
        } else if editor_focus.is_focused(window) {
            // A blank click has no new focus owner. Let its target run first,
            // then recover only the abandoned editor's focus, never another control's.
            cx.on_next_frame(window, move |this, window, cx| {
                if this.pane_rename.is_none()
                    && this.tab_rename.is_none()
                    && (editor_focus.is_focused(window) || window.focused(cx).is_none())
                    && let Some(view) =
                        this.tabs.get(this.active).and_then(WorkspaceTab::focused_view)
                {
                    // Do not defer a second time: another callback may focus a new input.
                    window.focus(&view.read(cx).focus_handle(cx), cx);
                }
            });
        }
        cx.notify();
    }

    /// Saved-name history is available on reopening. After typing, let the input
    /// handle its normal text history instead of merging it with a prior rename.
    pub(super) fn restore_pane_name(
        &mut self,
        redo: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(edit) = self.pane_rename.as_mut().filter(|edit| !edit.local_edits) else {
            cx.propagate();
            return;
        };
        cx.stop_propagation();
        let next = if redo {
            (edit.saved_name_index + 1).min(edit.saved_names.len() - 1)
        } else {
            edit.saved_name_index.saturating_sub(1)
        };
        if next == edit.saved_name_index {
            return;
        }
        edit.saved_name_index = next;
        let name = edit.saved_names[next].clone().unwrap_or_default();
        edit.input.update(cx, |state, cx| state.set_value(name, window, cx));
        window.dispatch_action(Box::new(gpui_component::input::SelectAll), cx);
        cx.notify();
    }

    pub(super) fn cancel_pane_rename(
        &mut self,
        restore_focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pane_rename.take().is_none() {
            return;
        }
        if restore_focus {
            self.focus_active(window, cx);
        }
        cx.notify();
    }

    pub(super) fn forget_pane_rename(&mut self, pane_id: u64) {
        if self.pane_rename.as_ref().is_some_and(|edit| edit.pane_id == pane_id) {
            self.pane_rename = None;
        }
    }
}
