//! Opt-in Windows clipboard acceptance with an isolated settings file.

use super::*;

#[test]
#[ignore = "requires a Windows desktop, PEBREL_PASTE_QA_DIR and isolated PEBREL_CONFIG_DIR"]
fn native_command_dialog_paste() {
    let output = std::path::PathBuf::from(std::env::var_os("PEBREL_PASTE_QA_DIR").unwrap());
    let config = output.join("config");
    assert_eq!(
        std::env::var_os("PEBREL_CONFIG_DIR").map(std::path::PathBuf::from),
        Some(config.clone())
    );
    assert!(!output.join("ready").exists(), "use a fresh fixture directory");
    std::fs::create_dir_all(&config).unwrap();
    let settings_file = config.join("pebrel_settings.txt");
    std::fs::write(&settings_file,
        "language=en-US\nkeybind=ctrl+v:ReceiveChar\nkeybind=ctrl+shift+v:ReceiveChar\nkeybind=ctrl+alt+v:Paste\n").unwrap();
    let restore = std::env::var("PEBREL_PASTE_QA_RESTORE").as_deref() == Ok("1");
    let checked = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let after_run = checked.clone();
    gpui_platform::application().with_assets(crate::gpui_shell::assets::NebulaAssets).run(
        move |cx| {
            let hub = crate::runtime_api::RuntimeHub::new();
            gpui_component::init(cx);
            cx.set_global(crate::gpui_shell::config::Settings::load(
                nebula_settings::ThemeName::Nord,
            ));
            crate::gpui_shell::theme::apply_chrome_theme(cx);
            crate::gpui_shell::math_view::register(cx);
            crate::gpui_shell::file_editor::init(cx);
            super::super::init(cx);
            windowing::initialize(cx, hub.clone());
            let mut workspace = None;
            let handle = cx
                .open_window(
                    gpui::WindowOptions {
                        window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds::new(
                            gpui::point(px(60.0), px(60.0)),
                            gpui::size(px(1000.0), px(760.0)),
                        ))),
                        focus: false,
                        show: true,
                        ..Default::default()
                    },
                    |window, cx| {
                        let view = cx.new(|cx| {
                            NebulaWorkspace::new(
                                window,
                                None,
                                None,
                                1,
                                hub,
                                windowing::WorkspaceStartup::Empty,
                                windowing::WindowRole::Regular,
                                cx,
                            )
                        });
                        workspace = Some(view.clone());
                        cx.new(|cx| Root::new(view, window, cx))
                    },
                )
                .unwrap();
            let workspace = workspace.unwrap();
            cx.spawn(async move |cx| {
                cx.background_executor().timer(std::time::Duration::from_millis(500)).await;
                cx.update_window(handle.into(), |_, window, cx| {
                    workspace.update(cx, |workspace, cx| {
                        if restore {
                            std::fs::write(&settings_file, "language=en-US\n").unwrap();
                            workspace.apply_custom_keybinds(cx);
                        }
                        workspace.open_saved_command_editor(None, window, cx);
                    });
                    let _ = window.draw(cx);
                })
                .unwrap();
                cx.background_executor().timer(std::time::Duration::from_millis(200)).await;
                let name = "Clipboard fixture";
                let command = "echo native_clipboard\necho second_line";
                cx.update_window(handle.into(), |_, window, cx| {
                    let previous = cx.read_from_clipboard();
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(name.into()));
                    window.dispatch_keystroke(gpui::Keystroke::parse("ctrl-v").unwrap(), cx);
                    window.dispatch_keystroke(gpui::Keystroke::parse("tab").unwrap(), cx);
                    let _ = window.draw(cx);
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(command.into()));
                    window.dispatch_keystroke(gpui::Keystroke::parse("ctrl-shift-v").unwrap(), cx);
                    let _ = window.draw(cx);
                    // Restore only while our fixture text still owns the clipboard.
                    if cx.read_from_clipboard().and_then(|item| item.text()).as_deref()
                        == Some(command)
                    {
                        cx.write_to_clipboard(
                            previous
                                .unwrap_or_else(|| gpui::ClipboardItem::new_string(String::new())),
                        );
                    }
                })
                .unwrap();
                std::fs::write(
                    output.join("expected.json"),
                    serde_json::to_vec(&serde_json::json!({
                        "name": name, "command": command, "restored": restore
                    }))
                    .unwrap(),
                )
                .unwrap();
                std::fs::write(output.join("ready"), std::process::id().to_string()).unwrap();
                for _ in 0..150 {
                    if output.join("capture-complete").exists() {
                        break;
                    }
                    cx.background_executor().timer(std::time::Duration::from_millis(200)).await;
                }
                checked
                    .store(output.join("verified").exists(), std::sync::atomic::Ordering::SeqCst);
                let _ = cx.update_window(handle.into(), |_, window, _| window.remove_window());
                cx.update(|cx| cx.quit());
            })
            .detach();
        },
    );
    assert!(
        after_run.load(std::sync::atomic::Ordering::SeqCst),
        "native input values were not verified"
    );
}
