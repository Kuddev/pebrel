//! Opt-in native visual fixture; no terminal or real saved command is opened.

use super::*;

#[test]
#[ignore = "requires a native desktop and PEBREL_COMMAND_QA_DIR"]
fn native_command_bulk_preview() {
    let output = std::path::PathBuf::from(std::env::var_os("PEBREL_COMMAND_QA_DIR").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    assert!(!output.join("ready").exists(), "use a fresh fixture directory");
    let mut saved =
        crate::saved_commands::SavedCommands::load_from(&output.join("commands.json")).unwrap();
    let first = saved.insert("Build project", "cargo build --locked", false).unwrap();
    saved.insert("Run focused tests", "cargo test --locked", false).unwrap();
    let theme = if std::env::var("PEBREL_COMMAND_QA_THEME").as_deref() == Ok("light") {
        nebula_settings::ThemeName::Paper
    } else {
        nebula_settings::ThemeName::Nord
    };
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
            let handle = cx
                .open_window(
                    gpui::WindowOptions {
                        window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds::new(
                            gpui::point(px(60.0), px(60.0)),
                            gpui::size(px(1000.0), px(720.0)),
                        ))),
                        focus: true,
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
                        view.update(cx, |this, cx| {
                            this.saved_commands = saved;
                            this.toggle_command_manager(window, cx);
                            this.command_manager_selection.selecting = true;
                            this.command_manager_selection.ids.insert(first.id);
                            if std::env::var("PEBREL_COMMAND_QA_STATE").as_deref() == Ok("confirm")
                            {
                                let owner = cx.entity().downgrade();
                                window.defer(cx, move |window, cx| {
                                    let _ = owner.update(cx, |this, cx| {
                                        this.open_command_deletion(Deletion::All, window, cx);
                                    });
                                });
                            }
                        });
                        cx.new(|cx| Root::new(view, window, cx))
                    },
                )
                .unwrap();
            cx.spawn(async move |cx| {
                cx.background_executor().timer(std::time::Duration::from_millis(800)).await;
                cx.update_window(handle.into(), |_, window, cx| {
                    let _ = window.draw(cx);
                })
                .unwrap();
                std::fs::write(output.join("ready"), std::process::id().to_string()).unwrap();
                for _ in 0..150 {
                    if output.join("capture-complete").exists() {
                        break;
                    }
                    cx.background_executor().timer(std::time::Duration::from_millis(200)).await;
                }
                let _ = cx.update_window(handle.into(), |_, window, _| window.remove_window());
                cx.update(|cx| cx.quit());
            })
            .detach();
        },
    );
}
