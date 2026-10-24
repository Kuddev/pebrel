//! Opt-in native visual fixture; no terminal or real saved command is opened.

use super::*;

#[test]
#[ignore = "requires a native desktop and PEBREL_COMMAND_QA_DIR"]
fn native_command_bulk_preview() {
    native_command_preview(false);
}

#[test]
#[ignore = "requires a native desktop, PEBREL_COMMAND_QA_DIR and isolated PEBREL_CONFIG_DIR"]
fn native_command_order_preview() {
    native_command_preview(true);
}

fn native_command_preview(ordering: bool) {
    let output = std::path::PathBuf::from(std::env::var_os("PEBREL_COMMAND_QA_DIR").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    assert!(!output.join("ready").exists(), "use a fresh fixture directory");
    let mut saved =
        crate::saved_commands::SavedCommands::load_from(&output.join("commands.json")).unwrap();
    let first = saved.insert("Build project", "cargo build --locked", false).unwrap();
    let second = saved.insert("Run focused tests", "cargo test --locked", false).unwrap();
    let order_group = if ordering {
        saved.create_group("Build and test · 构建与测试").unwrap();
        saved.create_group("Remote tools · 远程工具").unwrap();
        let group = saved.groups()[0].id.clone();
        saved.move_to_group(&first.id, Some(&group)).unwrap();
        saved.move_to_group(&second.id, Some(&group)).unwrap();
        saved
            .update(
                &first.id,
                "Build the project with a long name · 构建项目 🚀",
                "cargo build --locked",
                false,
            )
            .unwrap();
        Some(group)
    } else {
        None
    };
    let theme = if std::env::var("PEBREL_COMMAND_QA_THEME").as_deref() == Ok("light") {
        nebula_settings::ThemeName::Paper
    } else {
        nebula_settings::ThemeName::Nord
    };
    if ordering {
        let config = output.join("config");
        assert_eq!(
            std::env::var_os("PEBREL_CONFIG_DIR").map(std::path::PathBuf::from),
            Some(config.clone())
        );
        std::fs::create_dir_all(&config).unwrap();
        let language = if std::env::var("PEBREL_COMMAND_QA_LANGUAGE").as_deref() == Ok("zh-CN") {
            "zh-CN"
        } else {
            "en-US"
        };
        std::fs::write(
            config.join("pebrel_settings.txt"),
            format!(
                "language={language}\ntheme={}\nfollow_system_theme=0\ntray=0\n",
                theme.prompt_name()
            ),
        )
        .unwrap();
    }
    let width = std::env::var("PEBREL_COMMAND_QA_WIDTH")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|width| (320..=1600).contains(width))
        .unwrap_or(1000);
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
                            gpui::size(
                                px(f32::from(width)),
                                px(if ordering { 480.0 } else { 720.0 }),
                            ),
                        ))),
                        focus: true,
                        show: true,
                        window_min_size: ordering.then_some(gpui::size(px(320.0), px(300.0))),
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
                            if let Some(group) = order_group {
                                if std::env::var("PEBREL_COMMAND_QA_STATE").as_deref()
                                    == Ok("folder")
                                {
                                    this.enter_command_group(Some(group), window, cx);
                                } else if std::env::var("PEBREL_COMMAND_QA_STATE").as_deref()
                                    == Ok("search")
                                {
                                    this.command_manager_input.update(cx, |input, cx| {
                                        input.set_value("Build", window, cx)
                                    });
                                }
                            } else {
                                this.command_manager_selection.selecting = true;
                                this.command_manager_selection.ids.insert(first.id);
                            }
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
                    if ordering {
                        assert!(crate::gpui_shell::reveal_native_window(window));
                        // Workspace startup fits its default terminal grid first.
                        window.resize(gpui::size(px(f32::from(width)), px(480.0)));
                    }
                    let _ = window.draw(cx);
                })
                .unwrap();
                cx.background_executor().timer(std::time::Duration::from_millis(200)).await;
                cx.update_window(handle.into(), |_, window, cx| {
                    let _ = window.draw(cx);
                    if ordering {
                        let actual = window.bounds().size;
                        std::fs::write(
                            output.join("viewport.json"),
                            format!(
                                "{{\"width\":{},\"height\":{},\"scale\":{}}}",
                                f32::from(actual.width),
                                f32::from(actual.height),
                                window.scale_factor()
                            ),
                        )
                        .unwrap();
                        if std::env::var("PEBREL_COMMAND_QA_STATE").as_deref() == Ok("focus") {
                            for _ in 0..3 {
                                window.focus_next(cx);
                            }
                            window.dispatch_event(
                                gpui::PlatformInput::KeyDown(gpui::KeyDownEvent {
                                    keystroke: gpui::Keystroke::parse("enter").unwrap(),
                                    is_held: false,
                                    prefer_character_input: false,
                                }),
                                cx,
                            );
                            let _ = window.draw(cx);
                        } else if let Some(row) =
                            window.debug_bounds_for_selector("saved-command-row-0")
                        {
                            std::fs::write(
                                output.join("hover.json"),
                                format!(
                                    "{{\"x\":{},\"y\":{}}}",
                                    f32::from(row.center().x),
                                    f32::from(row.center().y)
                                ),
                            )
                            .unwrap();
                            window.dispatch_event(
                                gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                                    position: row.center(),
                                    pressed_button: None,
                                    modifiers: gpui::Modifiers::default(),
                                }),
                                cx,
                            );
                            let _ = window.draw(cx);
                        }
                    }
                })
                .unwrap();
                cx.background_executor().timer(std::time::Duration::from_millis(200)).await;
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
