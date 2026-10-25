//! 窗格拥有的阅读状态；关闭浮层即取消后台任务，不保存历史文本副本。
mod output_hover;
mod text;
pub(super) use output_hover::{OutputHover, OutputTarget};
pub(super) mod worker;

use crate::gpui_shell::copy_feedback::CopyFeedback;
use gpui::{Entity, Subscription, Task};
use gpui_component::input::InputState;
use nebula_terminal::term::reading::{CompletedCommand, ReadingError, ReadingSpan};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
pub(super) use text::{MAX_QUERY_BYTES, SearchHit};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ReadingIssue {
    Changed,
    Unavailable,
    QueryTooLong,
    CellTooLarge,
    CopyTooLarge,
    EmptyOutput,
}
impl From<ReadingError> for ReadingIssue {
    fn from(error: ReadingError) -> Self {
        match error {
            ReadingError::BudgetTooSmall => Self::CellTooLarge,
            ReadingError::UnsupportedScreen => Self::Unavailable,
            _ => Self::Changed,
        }
    }
}

pub(super) struct ReadingPanel {
    pub input: Entity<InputState>,
    pub focus_line: crate::gpui_shell::motion::FocusUnderline,
    pub feedback: Entity<CopyFeedback>,
    pub _input_events: Subscription,
    pub _feedback_events: Subscription,
    pub scope: Option<u64>,
    pub source: Option<ReadingSpan>,
    pub hits: Vec<SearchHit>,
    pub selected: Option<usize>,
    pub match_case: bool,
    pub running: bool,
    pub truncated: bool,
    pub notice: Option<ReadingIssue>,
    pub commands: Vec<CompletedCommand>,
    pub commands_open: bool,
    pub highlight: Option<ReadingSpan>,
}

#[derive(Default)]
pub(super) struct ReadingState {
    pub output_hover: OutputHover,
    pub panel: Option<ReadingPanel>,
    pub query_generation: Arc<AtomicU64>,
    pub copy_generation: Arc<AtomicU64>,
    pub query_task: Option<Task<()>>,
    pub copy_task: Option<Task<()>>,
    pub navigation_task: Option<Task<()>>,
    pub navigation_epoch: u64,
}
impl ReadingState {
    pub fn cancel_query(&mut self) -> u64 {
        self.navigation_task = None;
        self.navigation_epoch = self.navigation_epoch.wrapping_add(1);
        self.query_task = None;
        self.query_generation.fetch_add(1, Ordering::AcqRel).wrapping_add(1)
    }
    pub fn cancel_copy(&mut self) -> u64 {
        self.copy_task = None;
        self.copy_generation.fetch_add(1, Ordering::AcqRel).wrapping_add(1)
    }
}
impl Drop for ReadingState {
    fn drop(&mut self) {
        self.query_generation.fetch_add(1, Ordering::Release);
        self.copy_generation.fetch_add(1, Ordering::Release);
    }
}
