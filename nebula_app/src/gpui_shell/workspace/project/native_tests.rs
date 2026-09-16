//! Explicit Windows acceptance: real executor, real PTYs, synthetic directories.
//! The directory-picker boundary is supplied a result so no modal dialog steals
//! focus from the user's desktop. Deterministic tests exercise the actual button,
//! path prompt options, cancellation and keyboard route separately.

use super::*;
use gpui::{Bounds, WindowBounds, WindowOptions, point};
use nebula_terminal::event::Notify as _;
use std::sync::Mutex;

#[test]
#[ignore = "requires a Windows desktop and a fresh PEBREL_PROJECT_QA_DIR"]
fn native_project_folder_opens_a_real_pty_without_changing_the_existing_tab() {
    let output = PathBuf::from(std::env::var_os("PEBREL_PROJECT_QA_DIR").expect("QA directory"));
    assert!(output.is_absolute(), "use an absolute QA directory");
    assert_eq!(
        std::env::var_os("PEBREL_CONFIG_DIR").map(PathBuf::from),
        Some(output.join("config")),
        "native QA must never load or save the user's normal configuration",
    );
    let theme = std::env::var("PEBREL_PROJECT_QA_THEME").unwrap_or_else(|_| "Nord".into());
    let theme = nebula_settings::ThemeName::from_prompt_name(&theme).expect("built-in theme");
    let top = std::env::var("PEBREL_PROJECT_QA_LAYOUT").is_ok_and(|value| value == "top");
    let width: f32 = std::env::var("PEBREL_PROJECT_QA_WIDTH")
        .unwrap_or_else(|_| "1080".into())
        .parse()
        .expect("width");
    let original_directory = output.join("original");
    let project_directory = output.join("project 中文 with spaces");
    let marker = output.join("observed-cwd.txt");
    let ready = output.join("ready.json");
    assert!(!marker.exists() && !ready.exists(), "use a fresh QA directory");
    std::fs::create_dir_all(&original_directory).unwrap();
    std::fs::create_dir_all(&project_directory).unwrap();
    let outcome = Arc::new(Mutex::new(None));
    let after_run = outcome.clone();

    gpui_platform::application().with_assets(crate::gpui_shell::assets::NebulaAssets).run(
        move |cx| {
            gpui_component::init(cx);
            crate::gpui_shell::scientific_render::init(cx);
            super::super::init(cx);
            let hub = crate::runtime_api::RuntimeHub::new();
            super::super::windowing::initialize(cx, hub.clone());
            let mut settings = crate::gpui_shell::config::Settings::load(theme);
            settings.ui_language = crate::display::UiLanguage::EnUs;
            settings.shell_id = Some("cmd".into());
            cx.set_global(settings);
            crate::gpui_shell::theme::apply_chrome_theme(cx);
            let mut workspace = None;
            let initial_directory = original_directory.clone();
            let handle = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                            point(px(60.0), px(70.0)),
                            size(px(width), px(580.0)),
                        ))),
                        titlebar: Some(TitleBar::title_bar_options()),
                        focus: false,
                        show: true,
                        ..Default::default()
                    },
                    |window, cx| {
                        let view = cx.new(|cx| {
                            let mut workspace = NebulaWorkspace::new(
                                window,
                                None,
                                None,
                                1,
                                hub,
                                super::super::windowing::WorkspaceStartup::Empty,
                                super::super::windowing::WindowRole::Regular,
                                cx,
                            );
                            workspace.tabs_position = if top {
                                nebula_settings::TabsPositionName::Top
                            } else {
                                nebula_settings::TabsPositionName::Sidebar
                            };
                            workspace.add_terminal_at(Some(initial_directory), None, window, cx);
                            workspace
                        });
                        workspace = Some(view.clone());
                        window.resize(size(px(width), px(580.0)));
                        cx.new(|cx| Root::new(view, window, cx))
                    },
                )
                .unwrap();
            let workspace = workspace.unwrap();
            cx.spawn(async move |cx| {
                let result: Result<(), String> = async {
                    cx.background_executor().timer(Duration::from_millis(500)).await;
                    let original = cx
                        .update_window(handle.into(), |_, window, cx| {
                            workspace.update(cx, |view, cx| {
                                let original = view.tabs[0].focused_view().unwrap().clone();
                                view.open_project_selection(
                                    crate::gpui_shell::workspace::shell_launch::configured_local_launch(cx),
                                    std::future::ready(Ok(Some(project_directory.clone()))),
                                    window,
                                    cx,
                                );
                                original
                            })
                        })
                        .map_err(|error| error.to_string())?;

                    let mut selected = None;
                    for _ in 0..100 {
                        selected = cx
                            .update_window(handle.into(), |_, _, cx| {
                                let view = workspace.read(cx);
                                (view.tabs.len() == 2 && view.project_picker.is_none())
                                    .then(|| view.tabs[view.active].focused_view().unwrap().clone())
                            })
                            .map_err(|error| error.to_string())?;
                        if selected.is_some() {
                            break;
                        }
                        cx.background_executor().timer(Duration::from_millis(50)).await;
                    }
                    let selected = selected.ok_or("project tab did not open")?;
                    let marker_command =
                        format!("cmd.exe /d /u /c cd > \"{}\"\r", marker.display());
                    cx.update_window(handle.into(), |_, _, cx| -> Result<(), String> {
                        let terminal = selected.read(cx);
                        if terminal.cwd != project_directory.to_string_lossy()
                            || selected.entity_id() == original.entity_id()
                            || original.read(cx).cwd != original_directory.to_string_lossy()
                        {
                            return Err("project launch changed the wrong tab or directory".into());
                        }
                        let session = terminal.session.as_ref().ok_or("new terminal has no PTY")?;
                        if session.shell_pid == 0 {
                            return Err("new shell has no process".into());
                        }
                        // Only this fixture's new PTY receives the diagnostic command.
                        // Its output file is an absolute path under the explicit QA directory.
                        session.notifier.notify(marker_command.into_bytes());
                        Ok(())
                    })
                    .map_err(|error| error.to_string())??;

                    let mut observed = None;
                    for _ in 0..100 {
                        let marker = marker.clone();
                        observed = cx
                            .background_executor()
                            .spawn(async move {
                                std::fs::read(marker).ok().and_then(|bytes| {
                                    if bytes.len() % 2 != 0 {
                                        return None;
                                    }
                                    let wide = bytes
                                        .chunks_exact(2)
                                        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                                        .collect::<Vec<_>>();
                                    String::from_utf16(&wide)
                                        .ok()
                                        .filter(|text| !text.trim().is_empty())
                                })
                            })
                            .await;
                        if observed.is_some() {
                            break;
                        }
                        cx.background_executor().timer(Duration::from_millis(100)).await;
                    }
                    let observed = observed.ok_or("PTY did not report its working directory")?;
                    let expected = project_directory.to_string_lossy().replace('\\', "/");
                    if !observed.trim().replace('\\', "/").eq_ignore_ascii_case(&expected) {
                        return Err(format!("actual PTY cwd differs: {observed:?}"));
                    }
                    cx.update_window(handle.into(), |_, window, cx| {
                        let focus = workspace.read(cx).project_focus.clone();
                        focus.focus(window, cx);
                        let _ = window.draw(cx);
                    })
                    .map_err(|error| error.to_string())?;
                    cx.update_window(handle.into(), |_, _, cx| -> Result<(), String> {
                        let expected_layout = if top {
                            nebula_settings::TabsPositionName::Top
                        } else {
                            nebula_settings::TabsPositionName::Sidebar
                        };
                        if workspace.read(cx).tabs_position != expected_layout
                            || crate::gpui_shell::theme::effective_theme_name(cx) != theme
                            || cx.theme().is_dark()
                                == crate::gpui_shell::theme::chrome_theme(theme).skin().is_light
                        {
                            return Err(
                                "startup preferences replaced the requested QA theme/layout".into(),
                            );
                        }
                        Ok(())
                    })
                    .map_err(|error| error.to_string())??;
                    std::fs::write(
                        &ready,
                        serde_json::to_vec(&serde_json::json!({
                            "pid": std::process::id(), "theme": theme.prompt_name(),
                            "layout": if top { "top" } else { "sidebar" }, "width": width,
                            "native_pty_cwd_verified": true, "existing_tab_unchanged": true,
                        }))
                        .unwrap(),
                    )
                    .map_err(|error| error.to_string())?;
                    for _ in 0..100 {
                        if output.join("capture-complete").exists() {
                            break;
                        }
                        cx.background_executor().timer(Duration::from_millis(200)).await;
                    }
                    Ok(())
                }
                .await;
                let _ = cx.update_window(handle.into(), |_, _, cx| {
                    for tab in &workspace.read(cx).tabs {
                        if let WorkspaceTab::Terminal { panes, .. } = tab {
                            for pane in panes {
                                pane.view.read(cx).shutdown();
                            }
                        }
                    }
                });
                drop(workspace);
                *outcome.lock().unwrap() = Some(result);
                let _ = cx.update_window(handle.into(), |_, window, _| window.remove_window());
                cx.update(|cx| cx.quit());
            })
            .detach();
        },
    );
    assert_eq!(*after_run.lock().unwrap(), Some(Ok(())));
}
