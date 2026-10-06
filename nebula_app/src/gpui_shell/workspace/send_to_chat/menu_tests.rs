use super::*;
use crate::gpui_shell::terminal::view::TerminalLaunch;
use crate::gpui_shell::workspace::{TabMeta, windowing};
use gpui::{AssetSource as _, TestAppContext, VisualTestContext, point, size};
use gpui_component::Root;
use nebula_split::SplitTree;

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

#[gpui::test]
fn terminal_menu_renders_without_a_selection_and_escape_restores_focus(cx: &mut TestAppContext) {
    let paste_icon = crate::gpui_shell::assets::NebulaAssets
        .load(crate::gpui_shell::assets::nav::CLIPBOARD_PASTE)
        .unwrap()
        .expect("paste icon must be embedded in the product asset source");
    assert!(std::str::from_utf8(&paste_icon).unwrap().contains("viewBox=\"0 0 24 24\""));
    let hub = crate::runtime_api::RuntimeHub::new();
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::gpui_shell::math_view::register(cx);
        crate::gpui_shell::file_editor::init(cx);
        crate::gpui_shell::workspace::init(cx);
        windowing::initialize(cx, hub.clone());
    });
    let directory = tempfile::tempdir().unwrap();
    let program = directory.path().join("pebrel-test-missing-shell");
    let mut result = None;
    let (_, window) = cx.add_window_view(|window, cx| {
        let workspace = cx.new(|cx| {
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
        let (source, pane_id) = workspace.update(cx, |workspace, cx| {
            let pane = workspace.new_pane(
                (80, 24),
                TerminalLaunch::Local {
                    cwd: Some(directory.path().to_path_buf()),
                    shell: Some(nebula_terminal::tty::Shell::new(
                        program.to_string_lossy().into_owned(),
                        vec![],
                    )),
                    shell_name: None,
                },
                None,
                window,
                cx,
            );
            let source = pane.view.clone();
            let id = pane.id;
            workspace.insert_tab_at(
                0,
                WorkspaceTab::Terminal {
                    panes: vec![pane],
                    tree: SplitTree::leaf(id),
                    focused: id,
                    zoomed: false,
                    broadcast: false,
                },
                TabMeta::default(),
            );
            workspace.focus_active(window, cx);
            (source, id)
        });
        result = Some((workspace.clone(), source, pane_id));
        Root::new(workspace, window, cx)
    });
    let mut cx = window.clone();
    let (workspace, source, pane_id) = result.unwrap();
    cx.simulate_resize(size(px(1000.0), px(700.0)));
    draw(&mut cx);
    for _ in 0..2 {
        cx.update(|window, cx| {
            source.read(cx).focus_handle(cx).focus(window, cx);
            workspace.update(cx, |workspace, cx| {
                workspace.open_terminal_selection_context_menu(
                    source.clone(),
                    pane_id,
                    point(px(400.0), px(150.0)),
                    String::new(),
                    window,
                    cx,
                );
            });
        });
        draw(&mut cx);
        let bounds = cx.debug_bounds("terminal-selection-context-menu").unwrap();
        assert!(bounds.size.width > px(0.0) && bounds.size.height > px(0.0));
        assert!(bounds.bottom() <= px(700.0));
        assert!(
            workspace.read_with(&cx, |workspace, _| workspace.selection_context_menu.is_some())
        );
        cx.simulate_keystrokes("escape");
        draw(&mut cx);
        assert!(
            workspace.read_with(&cx, |workspace, _| workspace.selection_context_menu.is_none())
        );
        cx.update(|window, cx| assert!(source.read(cx).focus_handle(cx).is_focused(window)));
        assert_eq!(workspace.read_with(&cx, |workspace, _| workspace.tabs.len()), 1);
    }
}
