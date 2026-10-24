use super::*;
use crate::gpui_shell::terminal::view::{TerminalLaunch, TerminalViewEvent};
use crate::gpui_shell::workspace::{TabMeta, windowing};
use gpui::{
    AssetSource as _, Modifiers, MouseButton, TestAppContext, VisualTestContext, point, size,
};
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
fn terminal_menu_restores_focus_before_paste_input_and_after_escape(cx: &mut TestAppContext) {
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

    let receiver = source.update(&mut cx, |view, _| {
        // The local fake session reads only GPUI's test clipboard, never the
        // user's native CF_HDROP payload. File-paste focus guards are tested
        // separately with the same TerminalView input lifetime.
        assert!(view.ssh_destination.is_none());
        view.install_completion_test_session()
    });
    let observed = std::rc::Rc::new(std::cell::Cell::new(false));
    let pasted = observed.clone();
    let _subscription = cx.update(|window, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string(r"C:\copied-as-text.txt".into()));
        workspace.update(cx, |_, cx| {
            cx.subscribe_in(&source, window, move |workspace, source, event, window, cx| {
                if matches!(event, TerminalViewEvent::UserInput(_)) {
                    assert!(workspace.selection_context_menu.is_some(), "observe before dismissal");
                    assert!(
                        source.read(cx).focus_handle.is_focused(window),
                        "menu Paste must restore terminal focus before processing its payload"
                    );
                    pasted.set(true);
                }
            })
        })
    });
    draw(&mut cx);
    let position = point(px(400.0), px(150.0));
    let modifiers = Modifiers { control: true, ..Modifiers::default() };
    cx.simulate_mouse_down(position, MouseButton::Right, modifiers);
    cx.simulate_mouse_up(position, MouseButton::Right, modifiers);
    draw(&mut cx);
    assert!(cx.debug_bounds("terminal-selection-context-menu").is_some());
    cx.update(|window, cx| assert!(!source.read(cx).focus_handle.is_focused(window)));
    // The first Down selects Copy; the second selects the actual Paste item.
    cx.simulate_keystrokes("down down enter");
    draw(&mut cx);
    assert!(observed.get(), "the real menu item must enter the normal paste route");
    let bytes: Vec<u8> = receiver
        .try_iter()
        .filter_map(|message| match message {
            nebula_terminal::event_loop::Msg::Input(bytes) => Some(bytes.into_owned()),
            _ => None,
        })
        .flatten()
        .collect();
    assert_eq!(bytes, br"C:\copied-as-text.txt");
    assert!(workspace.read_with(&cx, |workspace, _| workspace.selection_context_menu.is_none()));
}
