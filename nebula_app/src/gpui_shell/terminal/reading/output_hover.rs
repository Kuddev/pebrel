//! 悬浮复制只持有当前命中和最后一次操作，不建立输出历史缓存。
use super::ReadingIssue;
use crate::gpui_shell::copy_feedback::CopyFeedback;
use gpui::{Entity, Subscription, Task};
use nebula_terminal::term::reading::ReadingSpan;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::gpui_shell::terminal) struct OutputTarget {
    pub id: u64,
    pub span: ReadingSpan,
}

#[derive(Default)]
pub(in crate::gpui_shell::terminal) struct OutputHover {
    pub hovered: Option<OutputTarget>,
    pub viewport: Option<(i64, usize, usize)>,
    /// 操作反馈保留在原结果上；指针已到另一结果时不能把对勾搬过去。
    pub feedback_target: Option<OutputTarget>,
    pub feedback: Option<Entity<CopyFeedback>>,
    pub feedback_subscription: Option<Subscription>,
    pub notice: Option<ReadingIssue>,
    pub generation: Arc<AtomicU64>,
    pub task: Option<Task<()>>,
}
impl OutputHover {
    pub fn cancel_copy(&mut self) -> u64 {
        self.task = None;
        self.generation.fetch_add(1, Ordering::AcqRel).wrapping_add(1)
    }
    pub fn clear_targets(&mut self) {
        self.cancel_copy();
        self.hovered = None;
        self.viewport = None;
        self.feedback_target = None;
        self.notice = None;
    }
}
impl Drop for OutputHover {
    fn drop(&mut self) {
        self.generation.fetch_add(1, Ordering::Release);
    }
}
