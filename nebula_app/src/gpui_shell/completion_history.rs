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
