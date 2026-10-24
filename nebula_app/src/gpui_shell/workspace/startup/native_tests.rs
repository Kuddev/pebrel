//! Opt-in native visual fixture with isolated settings and no initial terminal.

use super::*;

#[test]
#[ignore = "requires a native desktop and PEBREL_WELCOME_QA_DIR"]
fn native_welcome_preview() {
    let output = std::path::PathBuf::from(std::env::var_os("PEBREL_WELCOME_QA_DIR").unwrap());
    let config = std::path::PathBuf::from(std::env::var_os("PEBREL_CONFIG_DIR").unwrap());
    assert!(output.is_absolute() && config.starts_with(&output), "use an isolated config");
    std::fs::create_dir_all(&output).unwrap();
    assert!(!output.join("ready").exists(), "use a fresh fixture directory");
    let theme = if std::env::var("PEBREL_WELCOME_QA_THEME").as_deref() == Ok("light") {
        nebula_settings::ThemeName::Paper
    } else {
        nebula_settings::ThemeName::Nord
    };
    let narrow = std::env::var("PEBREL_WELCOME_QA_NARROW").as_deref() == Ok("1");
    let preview_size =
        if narrow { gpui::size(px(460.0), px(400.0)) } else { gpui::size(px(1000.0), px(720.0)) };
    gpui_platform::application().with_assets(crate::gpui_shell::assets::NebulaAssets).run(
        move |cx| {
            let hub = crate::runtime_api::RuntimeHub::new();
            gpui_component::init(cx);
            cx.set_global(crate::gpui_shell::config::Settings::load(theme));
            crate::gpui_shell::theme::apply_chrome_theme(cx);
            crate::gpui_shell::math_view::register(cx);
            crate::gpui_shell::file_editor::init(cx);
            super::super::init(cx);
            windowing::initialize(cx, hub.clone());
            let mut workspace_out = None;
            let handle = cx
                .open_window(
                    gpui::WindowOptions {
                        window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds::new(
                            gpui::point(px(60.0), px(60.0)),
                            preview_size,
                        ))),
                        focus: true,
                        show: true,
                        ..Default::default()
                    },
                    |window, cx| {
                        window.set_window_title("Pebrel welcome preview");
                        let view = cx.new(|cx| {
                            NebulaWorkspace::new(
                                window,
                                None,
                                None,
                                1,
                                hub,
                                windowing::WorkspaceStartup::Configured {
                                    cwd: Some(output.clone()),
                                },
                                windowing::WindowRole::Regular,
                                cx,
                            )
                        });
                        view.update(cx, |this, cx| {
                            assert!(this.active_welcome().is_some());
                            assert!(this.active_terminal_pane_id().is_none());
                            if std::env::var("PEBREL_WELCOME_QA_STATE").as_deref() == Ok("ssh") {
                                this.on_welcome_event(
                                    &this.active_welcome().unwrap(),
                                    &WelcomeEvent::Ssh,
                                    window,
                                    cx,
                                );
                            }
                        });
                        workspace_out = Some(view.clone());
                        cx.new(|cx| Root::new(view, window, cx))
                    },
                )
                .unwrap();
            let workspace = workspace_out.unwrap();
            cx.spawn(async move |cx| {
                cx.background_executor().timer(std::time::Duration::from_millis(800)).await;
                cx.update_window(handle.into(), |_, window, cx| {
                    window.set_window_visible(true);
                    window.activate_window();
                    // The constructor fits its terminal grid; review the requested size after it.
                    window.resize(preview_size);
                    let _ = window.draw(cx);
                    if let Some(hwnd) = windowing::native_hwnd(window) {
                        std::fs::write(output.join("window-handle"), hwnd.to_string()).unwrap();
                    }
                })
                .unwrap();
                std::fs::write(output.join("window-ready"), std::process::id().to_string())
                    .unwrap();
                for _ in 0..100 {
                    if output.join("shown").exists() {
                        break;
                    }
                    cx.background_executor().timer(std::time::Duration::from_millis(100)).await;
                }
                assert!(output.join("shown").exists(), "show the owned window before input");
                cx.background_executor().timer(std::time::Duration::from_millis(300)).await;
                cx.update_window(handle.into(), |_, window, cx| {
                    assert_eq!(crate::gpui_shell::theme::effective_theme_name(cx), theme);
                    let actual = window.bounds().size;
                    assert!((f32::from(actual.width - preview_size.width)).abs() < 2.0);
                    assert!((f32::from(actual.height - preview_size.height)).abs() < 2.0);
                    std::fs::write(
                        output.join("layout.json"),
                        serde_json::json!({
                            "width": f32::from(actual.width),
                            "height": f32::from(actual.height),
                            "scale": window.scale_factor(),
                            "theme": theme.prompt_name(),
                        })
                        .to_string(),
                    )
                    .unwrap();
                    let _ = window.draw(cx);
                    let state = std::env::var("PEBREL_WELCOME_QA_STATE").unwrap_or_default();
                    if state.starts_with("hover") || state.starts_with("pressed") {
                        let selector =
                            if state.ends_with("ssh") { "welcome-ssh" } else { "welcome-local" };
                        let position = window.debug_bounds_for_selector(selector).unwrap().center();
                        window.dispatch_event(
                            gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                                position,
                                ..Default::default()
                            }),
                            cx,
                        );
                        let _ = window.draw(cx);
                        if state.starts_with("pressed") {
                            window.dispatch_event(
                                gpui::PlatformInput::MouseDown(gpui::MouseDownEvent {
                                    position,
                                    button: gpui::MouseButton::Left,
                                    click_count: 1,
                                    ..Default::default()
                                }),
                                cx,
                            );
                        }
                    }
                    if matches!(state.as_str(), "focus" | "local") {
                        window.dispatch_keystroke(gpui::Keystroke::parse("tab").unwrap(), cx);
                        let _ = window.draw(cx);
                    }
                    if state == "local" {
                        window.dispatch_keystroke(gpui::Keystroke::parse("enter").unwrap(), cx);
                        window.dispatch_event(
                            gpui::PlatformInput::KeyUp(gpui::KeyUpEvent {
                                keystroke: gpui::Keystroke::parse("enter").unwrap(),
                            }),
                            cx,
                        );
                    }
                })
                .unwrap();
                cx.background_executor().timer(std::time::Duration::from_millis(400)).await;
                cx.update(|cx| {
                    let view = workspace.read(cx);
                    if std::env::var("PEBREL_WELCOME_QA_STATE").as_deref() == Ok("local") {
                        assert_eq!(view.tabs.len(), 1);
                        let terminal = view.tabs[0].focused_view().unwrap().read(cx);
                        let shell_pid =
                            terminal.session.as_ref().expect("real PTY started").shell_pid;
                        assert!(shell_pid > 0);
                        std::fs::write(output.join("shell-pid"), shell_pid.to_string()).unwrap();
                    }
                });
                std::fs::write(output.join("ready"), std::process::id().to_string()).unwrap();
                for _ in 0..150 {
                    if output.join("capture-complete").exists() {
                        break;
                    }
                    cx.background_executor().timer(std::time::Duration::from_millis(200)).await;
                }
                let _ = cx.update_window(handle.into(), |_, window, cx| {
                    for tab in &workspace.read(cx).tabs {
                        if let WorkspaceTab::Terminal { panes, .. } = tab {
                            for pane in panes {
                                pane.view.read(cx).shutdown();
                            }
                        }
                    }
                    window.remove_window();
                });
                cx.update(|cx| cx.quit());
            })
            .detach();
        },
    );
}
