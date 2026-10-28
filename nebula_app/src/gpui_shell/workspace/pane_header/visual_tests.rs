//! Opt-in production-window fixture for headerless panes. Uses only a QA folder.

use super::*;
use crate::gpui_shell::terminal::view::TerminalLaunch;
use crate::gpui_shell::workspace::{TabMeta, windowing};
use gpui::AppContext as _;
use gpui_component::Root;

#[test]
#[ignore = "requires a native desktop and PEBREL_PANES_QA_DIR"]
fn native_sidebar_panes() {
    let output = std::path::PathBuf::from(std::env::var_os("PEBREL_PANES_QA_DIR").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    assert!(!output.join("ready").exists(), "use a fresh QA directory");
    let theme = if std::env::var("PEBREL_PANES_QA_THEME").as_deref() == Ok("light") {
        nebula_settings::ThemeName::Paper
    } else {
        nebula_settings::ThemeName::Nord
    };
    let width =
        std::env::var("PEBREL_PANES_QA_WIDTH").ok().and_then(|s| s.parse().ok()).unwrap_or(1100.0);
    gpui_platform::application().with_assets(crate::gpui_shell::assets::NebulaAssets).run(move |cx| {
        let hub = crate::runtime_api::RuntimeHub::new();
        gpui_component::init(cx);
        cx.set_global(crate::gpui_shell::config::Settings::load(theme));
        crate::gpui_shell::theme::apply_chrome_theme(cx);
        crate::gpui_shell::math_view::register(cx);
        crate::gpui_shell::file_editor::init(cx);
        super::super::init(cx);
        windowing::initialize(cx, hub.clone());
        let directory = output.clone();
        let handle = cx.open_window(gpui::WindowOptions {
            window_bounds: Some(gpui::WindowBounds::Windowed(Bounds::new(
                gpui::point(px(40.0), px(40.0)), size(px(width), px(720.0)),
            ))),
            focus: true, show: true, ..Default::default()
        }, |window, cx| {
            let workspace = cx.new(|cx| NebulaWorkspace::new(
                window, None, None, 1, hub, windowing::WorkspaceStartup::Empty,
                windowing::WindowRole::Regular, cx,
            ));
            workspace.update(cx, |workspace, cx| {
                workspace.tabs_position = nebula_settings::TabsPositionName::Sidebar;
                let panes = ["API server", "Unit tests", "Build", "Shell"].into_iter().enumerate().map(|(index, name)| {
                    let command = format!("printf '\\033[36m~/project\\033[0m  {name}\\n\\n$ cargo {}\\n\\nReady.\\n'; exec sleep 120", if index == 1 { "test" } else { "check" });
                    let mut pane = workspace.new_pane((80, 24), TerminalLaunch::Local {
                        cwd: Some(directory.clone()),
                        shell: Some(nebula_terminal::tty::Shell::new("/bin/sh".into(), vec!["-c".into(), command])),
                        shell_name: None,
                    }, None, window, cx);
                    pane.custom_name = Some(name.into());
                    pane
                }).collect::<Vec<_>>();
                let ids = panes.iter().map(|pane| pane.id).collect::<Vec<_>>();
                let mut tree = SplitTree::leaf(ids[0]);
                tree.split_leaf(ids[0], ids[1], SplitDirection::LeftRight, 0.5);
                tree.split_leaf(ids[0], ids[2], SplitDirection::TopBottom, 0.5);
                tree.split_leaf(ids[1], ids[3], SplitDirection::TopBottom, 0.5);
                workspace.insert_tab_at(0, WorkspaceTab::Terminal {
                    tree, panes, focused: ids[0], zoomed: false, broadcast: false,
                }, TabMeta { custom_name: Some("Development".into()), ..Default::default() });
                workspace.focus_active(window, cx);
                cx.notify();
            });
            cx.new(|cx| Root::new(workspace, window, cx))
        }).unwrap();
        cx.spawn(async move |cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(1200)).await;
            cx.update_window(handle.into(), |_, window, cx| { let _ = window.draw(cx); }).unwrap();
            std::fs::write(output.join("ready"), std::process::id().to_string()).unwrap();
            for _ in 0..250 {
                if output.join("capture-complete").exists() { break; }
                cx.background_executor().timer(std::time::Duration::from_millis(200)).await;
            }
            let _ = cx.update_window(handle.into(), |_, window, _| window.remove_window());
            cx.update(|cx| cx.quit());
        }).detach();
    });
}
