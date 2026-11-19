//! One confirmed clear operation across all windows of the resident application.

use gpui::{App, AppContext as _, Global};

#[derive(Default)]
pub(crate) enum ClearState {
    #[default]
    Idle,
    Running,
    Finished(Result<(), String>),
}

impl Global for ClearState {}

/// Keeps the history write gate equal to the current settings before any terminal
/// starts. Every settings update replaces this global, so open terminals follow
/// the switch at once; history files already on disk stay untouched.
pub(crate) fn init(cx: &mut App) {
    let apply = |cx: &mut App| {
        let persist = cx.global::<super::config::Settings>().command_history_persist;
        crate::nebula_history::set_persistence(persist);
    };
    apply(cx);
    cx.observe_global::<super::config::Settings>(apply).detach();
}

pub(crate) fn running(cx: &App) -> bool {
    matches!(cx.try_global::<ClearState>(), Some(ClearState::Running))
}

pub(crate) fn clear(cx: &mut App) {
    if running(cx) {
        return;
    }
    #[cfg(all(test, feature = "gpui-test-support"))]
    let operation = cx.try_global::<TestWorker>().map(|worker| (worker.0)());
    #[cfg(not(all(test, feature = "gpui-test-support")))]
    let operation: Option<futures::future::BoxFuture<'static, Result<(), String>>> = None;
    cx.set_global(ClearState::Running);
    let task = cx.background_executor().spawn(async move {
        match operation {
            Some(operation) => operation.await,
            None => crate::completion::clear_command_history(),
        }
    });
    // The application owns completion, so closing Settings cannot strand other
    // windows with old suggestions or a permanently disabled clear button.
    cx.spawn(async move |cx| {
        let result = task.await;
        let _ = cx.update(|cx| cx.set_global(ClearState::Finished(result)));
    })
    .detach();
}

#[cfg(all(test, feature = "gpui-test-support"))]
pub(crate) struct TestWorker(
    pub  std::sync::Arc<
        dyn Fn() -> futures::future::BoxFuture<'static, Result<(), String>> + Send + Sync,
    >,
);

#[cfg(all(test, feature = "gpui-test-support"))]
impl Global for TestWorker {}
