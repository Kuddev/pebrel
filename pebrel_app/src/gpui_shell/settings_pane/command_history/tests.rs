use super::*;
use futures::FutureExt as _;
use gpui::{Modifiers, TestAppContext, VisualTestContext, size};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

fn click(window: &mut VisualTestContext, selector: &'static str) {
    window.run_until_parked();
    window.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    let bounds = window.debug_bounds(selector).expect(selector);
    assert!(bounds.size.width > px(0.0) && bounds.size.height > px(0.0));
    window.simulate_click(bounds.center(), Modifiers::default());
    window.run_until_parked();
}

#[gpui::test]
fn command_history_clear_confirmation_cancel_busy_and_failure_feedback(cx: &mut TestAppContext) {
    let _lock = crate::gpui_shell::settings_fixture::lock_theme_studio();
    let calls = Arc::new(AtomicUsize::new(0));
    let (sender, receiver) = futures::channel::oneshot::channel();
    let receiver = Arc::new(Mutex::new(Some(receiver)));
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_reduce_motion(true);
        cx.set_global(crate::gpui_shell::config::Settings::load(ThemeName::Nord));
        let calls = calls.clone();
        cx.set_global(completion_history::TestWorker(Arc::new(move || {
            calls.fetch_add(1, Ordering::SeqCst);
            let receiver = receiver.lock().unwrap().take().unwrap();
            async move { receiver.await.unwrap() }.boxed()
        })));
    });
    let (_, window) = cx.add_window_view(|window, cx| {
        let pane = cx.new(|cx| SettingsPane::new(window, cx));
        pane.update(cx, |pane, _| pane.active_section = 2);
        let surface = cx.new(|_| HistorySurface(pane));
        gpui_component::Root::new(surface, window, cx)
    });
    window.simulate_resize(size(px(1150.0), px(1100.0)));
    click(window, "nebula-btn-command-history-clear");
    window.simulate_keystrokes("escape");
    window.run_until_parked();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    click(window, "nebula-btn-command-history-clear");
    click(window, "confirm-dialog-cancel");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    click(window, "nebula-btn-command-history-clear");
    click(window, "confirm-dialog-ok");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(window.update(|_, cx| completion_history::running(cx)));
    click(window, "nebula-btn-command-history-clear");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    sender.send(Err("fixture permission denied".into())).unwrap();
    window.run_until_parked();
    window.update(|window, cx| {
        assert!(matches!(cx.try_global::<ClearState>(), Some(ClearState::Finished(Err(error))) if error == "fixture permission denied"));
        window.refresh();
        window.draw(cx).clear(cx);
    });
    assert!(window.debug_bounds("command-history-status").is_some());
}

#[gpui::test]
fn command_history_clear_completes_after_origin_window_closes(cx: &mut TestAppContext) {
    let (sender, receiver) = futures::channel::oneshot::channel();
    let receiver = Arc::new(Mutex::new(Some(receiver)));
    cx.update(|cx| {
        cx.set_global(completion_history::TestWorker(Arc::new(move || {
            let receiver = receiver.lock().unwrap().take().unwrap();
            async move { receiver.await.unwrap() }.boxed()
        })));
    });
    let (_, window) = cx.add_window_view(|_, _| EmptyView);
    window.update(|window, cx| {
        completion_history::clear(cx);
        window.remove_window();
    });
    sender.send(Ok(())).unwrap();
    cx.executor().run_until_parked();
    cx.update(|cx| {
        assert!(matches!(cx.try_global::<ClearState>(), Some(ClearState::Finished(Ok(())))))
    });
}

struct EmptyView;
impl Render for EmptyView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

struct HistorySurface(Entity<SettingsPane>);
impl Render for HistorySurface {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(self.0.clone())
            .children(gpui_component::Root::render_dialog_layer(window, cx))
    }
}
