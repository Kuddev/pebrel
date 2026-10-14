//! Shared fixture for tests that read or write the real settings path.
//!
//! These rendered fixtures share the real settings path and theme library.
//! Readers must hold the same guard as Save/Apply tests so their before/after
//! snapshots cannot observe another fixture's writes or restoration cleanup.
//! Only this fixture group is serialized; the rest of the native suite stays parallel.
//!
//! Under nextest the in-process mutex is not enough on its own: each test runs in
//! its own process, so `.config/nextest.toml` repeats the same group as a serial
//! test-group. Both halves must stay in sync when a fixture joins the group.

/// Serializes every fixture that touches the real settings path.
pub(crate) fn lock_theme_studio() -> std::sync::MutexGuard<'static, ()> {
    static FIXTURES: std::sync::Mutex<()> = std::sync::Mutex::new(());
    FIXTURES.lock().unwrap_or_else(|error| error.into_inner())
}

/// Restores the settings file's original bytes on drop, removing it when it did not exist.
///
/// Any test that lets the product persist settings must hold [`lock_theme_studio`]
/// for as long as this guard lives.
pub(crate) struct SettingsBytesGuard {
    path: std::path::PathBuf,
    bytes: Option<Vec<u8>>,
}

impl SettingsBytesGuard {
    pub(crate) fn capture() -> Self {
        let path = nebula_settings::settings_path();
        Self { bytes: std::fs::read(&path).ok(), path }
    }
}

impl Drop for SettingsBytesGuard {
    fn drop(&mut self) {
        match &self.bytes {
            Some(bytes) => {
                let _ = std::fs::write(&self.path, bytes);
            },
            None => {
                let _ = std::fs::remove_file(&self.path);
            },
        }
    }
}

use gpui::{
    Context, Entity, IntoElement, ParentElement as _, Render, Styled as _, Window, deferred, div,
};
use gpui_component::Root;

use super::settings_pane::SettingsPane;

/// SettingsPane 发起弹窗请求，宿主像真实工作区一样提供模态绘制层。
pub(super) struct SettingsPaneHost {
    pub(super) pane: Entity<SettingsPane>,
}

impl Render for SettingsPaneHost {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(self.pane.clone())
            // 模态层高于功能浮层和组件弹出框，鼠标事件才能到达真正的确认按钮。
            .child(
                deferred(div().size_full().children(Root::render_dialog_layer(window, cx)))
                    .with_priority(10),
            )
    }
}
