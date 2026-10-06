//! GPUI 补齐适配：拥有任务生命周期，连接终端提交与列表交互。

#[cfg(test)]
pub(super) use crate::completion::history_hint_for_test;
pub(super) use crate::completion::{Cancellation, record_directory};
use crate::display::NebulaPaneState;

/// 视图释放任务时同时通知已开始的同步计算，不能仅丢弃最后的 UI 回填。
pub(super) struct Pending {
    _task: gpui::Task<()>,
    cancellation: Cancellation,
}

impl Pending {
    pub(super) fn new(task: gpui::Task<()>, cancellation: Cancellation) -> Self {
        Self { _task: task, cancellation }
    }

    #[cfg(test)]
    pub(super) fn cancellation(&self) -> Cancellation {
        self.cancellation.clone()
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

/// Enter 提交：命令进共享历史（与旧壳 `nebula_commit_line` 同一落点），
/// pane 状态清空等下一行。空行只清不记。
pub fn commit_line(state: &mut NebulaPaneState) {
    let line = state.screen_line.trim().to_owned();
    let committed = if line.is_empty() { state.line_buf.trim().to_owned() } else { line.clone() };
    if !line.is_empty() {
        crate::completion::record_command(&state.suggest_env.history_scope(), &line, &state.cwd);
        state.completion_submitted(&line);
    } else {
        state.completion_submitted(&state.line_buf.clone());
    }
    // 旧壳 `nebula_commit_line` 同一条：OSC 133;C 到达时 PTY 已把行缓冲清
    // 空，程序身份（侧栏 tab 图标）必须在 Enter 这一刻从屏幕真值捕获。
    // grid 读失败时退回按键镜像——取首 token 做身份已足够。
    state.last_committed = committed;
    crate::display::nebula_clear_line(state);
}

/// 弹窗列表是否正显示。
pub fn popup_active(state: &NebulaPaneState) -> bool {
    !state.completion_items.is_empty()
}

/// 弹窗高亮行循环移动。初始没有选中项；首次向任一方向导航都从首项进入，
/// 避免 Up 在无选择态直接跳到列表末尾。
pub fn popup_move(state: &mut NebulaPaneState, delta: isize) {
    state.completion_popup_move(delta);
}

/// 取走选中候选要键入的余量并关闭列表。
pub fn popup_take(state: &mut NebulaPaneState) -> Option<crate::display::NebulaCompletionItem> {
    state.completion_popup_take()
}

/// Esc 关闭列表；候选清空但重算键保留，列表在行变化前不会复开（与旧壳
/// `nebula_completion_popup_dismiss` 的缓存约定一致）。返回是否真的关了。
pub fn popup_dismiss(state: &mut NebulaPaneState) -> bool {
    state.completion_popup_dismiss()
}

#[cfg(test)]
mod tests {
    use super::{popup_active, popup_move, popup_take};
    use crate::display::{NebulaCompletionItem, NebulaCompletionKind, NebulaPaneState};

    fn popup_state() -> NebulaPaneState {
        let mut state = NebulaPaneState::default();
        state.completion_items = vec![
            NebulaCompletionItem {
                replace_chars: 0,
                replace_after_chars: 0,
                label: "git pull upstream".to_owned(),
                insert: " upstream".to_owned(),
                kind: NebulaCompletionKind::History,
            },
            NebulaCompletionItem {
                replace_chars: 0,
                replace_after_chars: 0,
                label: "git pull --rebase".to_owned(),
                insert: " --rebase".to_owned(),
                kind: NebulaCompletionKind::Command,
            },
        ];
        state
    }

    #[test]
    fn popup_does_not_accept_before_explicit_navigation() {
        let mut state = popup_state();
        assert!(popup_active(&state));
        assert_eq!(state.completion_selected, None);
        assert!(popup_take(&mut state).is_none());
        assert!(popup_active(&state));
    }

    #[test]
    fn popup_first_navigation_enters_at_the_first_item() {
        for delta in [-1, 1] {
            let mut state = popup_state();
            popup_move(&mut state, delta);
            assert_eq!(state.completion_selected, Some(0));
            assert_eq!(
                popup_take(&mut state).map(|item| item.insert).as_deref(),
                Some(" upstream")
            );
            assert!(!popup_active(&state));
        }
    }

    #[test]
    fn hovering_and_scrolling_do_not_change_the_keyboard_accept_target() {
        let mut state = popup_state();
        popup_move(&mut state, 1);
        let mut viewport = super::super::completion_viewport::CompletionViewport::default();
        viewport.hover((12.0, 28.0), Some(1));
        viewport.scroll(-28.0, 28.0, state.completion_items.len(), 1);
        assert_eq!(viewport.offset, 1);
        assert_eq!(state.completion_selected, Some(0));
        assert_eq!(popup_take(&mut state).map(|item| item.insert).as_deref(), Some(" upstream"));
    }
}
