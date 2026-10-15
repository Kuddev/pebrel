//! Native X11/Wayland lifecycle regression, run only with an isolated CI desktop.
use super::*;
use gpui::{MouseButton, MouseDownEvent, MouseUpEvent, PlatformInput};

struct ActivationFixture;
impl Render for ActivationFixture {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(gpui::black())
    }
}

fn pane_entities(workspace: &NebulaWorkspace) -> Vec<gpui::EntityId> {
    workspace
        .tabs
        .iter()
        .flat_map(|tab| match tab {
            WorkspaceTab::Terminal { panes, .. } => {
                panes.iter().map(|pane| pane.view.entity_id()).collect()
            },
            _ => Vec::new(),
        })
        .collect()
}

fn check_x11_mapping(backend: &str, id: u32, visible: bool) {
    if backend != "x11" {
        return;
    }
    let result = std::process::Command::new("xwininfo")
        .args(["-id", &id.to_string()])
        .env("LC_ALL", "C")
        .output()
        .unwrap();
    assert!(result.status.success(), "xwininfo failed");
    let text = String::from_utf8(result.stdout).unwrap();
    assert!(
        text.contains(if visible { "Map State: IsViewable" } else { "Map State: IsUnMapped" }),
        "{text}"
    );
}

#[test]
#[ignore = "requires X11/Wayland and isolated PEBREL_DESKTOP_QA_DIR/config"]
fn native_linux_silent_start_close_and_restore_keep_the_same_session() {
    assert_eq!(crate::platform::Platform::current(), crate::platform::Platform::Linux);
    let output = PathBuf::from(std::env::var_os("PEBREL_DESKTOP_QA_DIR").unwrap());
    assert!(output.is_absolute());
    assert_eq!(
        std::env::var_os("PEBREL_CONFIG_DIR").map(PathBuf::from),
        Some(output.join("config"))
    );
    std::fs::create_dir_all(output.join("config")).unwrap();
    let result_path = output.join("result.json");
    assert!(!result_path.exists());
    std::fs::write(output.join("config/pebrel_settings.txt"),
        "silent_start=1\ntray=1\nkeep_session=1\nrestore_session=0\nresume_ai=0\nbackground_image=\nlanguage=en-US\n").unwrap();
    gpui_platform::application()
        .with_assets(crate::gpui_shell::assets::NebulaAssets)
        .with_quit_mode(gpui::QuitMode::Explicit)
        .run(move |cx| {
            crate::gpui_shell::init(cx, None);
            initialize(cx, crate::runtime_api::RuntimeHub::new());
            let (_sender, receiver) = std::sync::mpsc::channel();
            let (_, workspace) = open_workspace_window(
                cx,
                WorkspaceStartup::RestoreOrDefault,
                None,
                Some(receiver),
                false,
                WindowRole::Regular,
            )
            .unwrap();
            let handle = cx.windows()[0];
            let (backend, id) = handle
                .update(cx, |_, window, _| {
                    crate::platform::window_chrome::native_test_window_id(window).unwrap()
                })
                .unwrap();
            std::fs::write(
                output.join("window.json"),
                serde_json::to_vec(&json!({"backend":backend,"id":id})).unwrap(),
            )
            .unwrap();
            cx.spawn(async move |cx| {
                cx.background_executor().timer(Duration::from_millis(400)).await;
                let panes = cx.update(|cx| {
                    assert!(workspace.read(cx).window_hidden);
                    let panes = pane_entities(workspace.read(cx));
                    assert!(!panes.is_empty());
                    panes
                });
                check_x11_mapping(backend, id, false);
                if backend == "x11" {
                    let property = std::process::Command::new("xprop")
                        .args(["-id", &id.to_string(), "WM_CLASS"])
                        .output()
                        .unwrap();
                    assert!(property.status.success());
                    assert!(
                        String::from_utf8(property.stdout)
                            .unwrap()
                            .contains(crate::brand::LINUX_APP_ID)
                    );
                }
                crate::gpui_shell::try_write_stderr(format_args!("PEBREL_DESKTOP_QA_SHOW"));
                cx.update(|cx| focus_notification(None, cx));
                cx.background_executor().timer(Duration::from_millis(400)).await;
                check_x11_mapping(backend, id, true);
                handle
                    .update(cx, |_, window, cx| {
                        assert!(!workspace.read(cx).window_hidden);
                        assert_eq!(pane_entities(workspace.read(cx)), panes);
                        window.refresh();
                        window.draw(cx).clear(cx);
                        let title = window.debug_bounds_for_selector("workspace-titlebar").unwrap();
                        // 固定组件的关闭按钮宽 34 px；使用真实标题栏布局，避免窗口边框偏移。
                        let position = point(title.right() - px(17.0), title.center().y);
                        window.dispatch_event(
                            PlatformInput::MouseDown(MouseDownEvent {
                                button: MouseButton::Left,
                                position,
                                click_count: 1,
                                ..Default::default()
                            }),
                            cx,
                        );
                        window.dispatch_event(
                            PlatformInput::MouseUp(MouseUpEvent {
                                button: MouseButton::Left,
                                position,
                                click_count: 1,
                                ..Default::default()
                            }),
                            cx,
                        );
                    })
                    .unwrap();
                cx.background_executor().timer(Duration::from_millis(400)).await;
                cx.update(|cx| {
                    assert!(cx.windows().contains(&handle), "titlebar bypassed the close guard");
                    assert!(workspace.read(cx).window_hidden, "close did not retain the workspace");
                    assert_eq!(pane_entities(workspace.read(cx)), panes);
                });
                check_x11_mapping(backend, id, false);
                crate::gpui_shell::try_write_stderr(format_args!("PEBREL_DESKTOP_QA_RESTORE"));
                cx.update(|cx| focus_notification(None, cx));
                cx.background_executor().timer(Duration::from_millis(400)).await;
                check_x11_mapping(backend, id, true);
                cx.update(|cx| {
                    assert!(!workspace.read(cx).window_hidden);
                    assert_eq!(pane_entities(workspace.read(cx)), panes);
                });
                if backend == "wayland" {
                    let cancelled = cx.update(|cx| {
                        let handle = cx
                            .open_window(
                                WindowOptions { show: false, focus: false, ..Default::default() },
                                |_, cx| cx.new(|_| ActivationFixture),
                            )
                            .unwrap();
                        handle
                            .update(cx, |_, window, _| {
                                window.activate_window_with_token("pebrel-qa-cancelled-activation");
                                assert!(window.set_window_visible(false));
                            })
                            .unwrap();
                        handle
                    });
                    cx.background_executor().timer(Duration::from_millis(400)).await;
                    cancelled.update(cx, |_, window, _| window.remove_window()).unwrap();
                }
                std::fs::write(
                    output.join("result.json"),
                    serde_json::to_vec(&json!({
                        "backend":backend,"id":id,"silent_start":true,"titlebar_close_guard":true,
                        "same_terminal_views_after_restore":true
                    }))
                    .unwrap(),
                )
                .unwrap();
                handle.update(cx, |_, window, _| window.remove_window()).unwrap();
                // 验收任务也持有 workspace；退出前释放它，让窗口删除先完成实体回收。
                drop(workspace);
                cx.update(|cx| cx.defer(|cx| cx.quit()));
            })
            .detach();
        });
    assert!(result_path.is_file());
}
