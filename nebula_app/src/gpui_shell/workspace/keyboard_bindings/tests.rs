use super::*;
#[test]
fn cleared_shortcut_reaches_terminal_and_can_be_restored_without_restart() {
    use crate::config::Action;
    use gpui::{KeyContext, Keymap, Keystroke};
    let contexts = [
        KeyContext::parse("Root").unwrap(),
        KeyContext::parse(crate::gpui_shell::terminal::KEY_CONTEXT).unwrap(),
    ];
    let input = [Keystroke::parse("ctrl-k").unwrap()];
    let original = custom_workspace_binding("ctrl+k", &Action::ToggleShellPicker).unwrap();
    let clear = workspace_binding_in_context(
        "ctrl+k",
        &Action::ReceiveChar,
        Some(crate::gpui_shell::terminal::KEY_CONTEXT),
    )
    .unwrap();
    let disabled = Keymap::new(vec![original.clone(), clear.clone()]);
    let (bindings, pending) = disabled.bindings_for_input(&input, &contexts);
    assert!(bindings.is_empty() && !pending, "No app action should consume the key");
    let restored = workspace_binding_in_context(
        "ctrl+k",
        &Action::ToggleShellPicker,
        Some(crate::gpui_shell::terminal::KEY_CONTEXT),
    )
    .unwrap();
    let restored = Keymap::new(vec![original, clear, restored]);
    let (bindings, pending) = restored.bindings_for_input(&input, &contexts);
    assert!(!pending);
    assert!(bindings[0].action().as_any().is::<ToggleShellPicker>());
}

#[cfg(feature = "gpui-test-support")]
mod dispatch {
    use super::*;
    use gpui::{FocusHandle, Keystroke, TestAppContext, VisualTestContext};
    use nebula_terminal::term::TermMode;

    fn press(combo: &str, cx: &mut VisualTestContext) {
        let keystroke = Keystroke::parse(combo).unwrap();
        cx.simulate_event(KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(gpui::KeyUpEvent { keystroke });
        cx.run_until_parked();
    }

    fn open_workspace(
        count: usize,
        cx: &mut TestAppContext,
    ) -> (tempfile::TempDir, Entity<NebulaWorkspace>, VisualTestContext) {
        let directory = tempfile::tempdir().unwrap();
        let paths: Vec<_> = (0..count)
            .map(|index| {
                let path = directory.path().join(format!("tab-{index}.txt"));
                std::fs::write(&path, "fixture\n").unwrap();
                path
            })
            .collect();
        let hub = crate::runtime_api::RuntimeHub::new();
        cx.update(|cx| {
            gpui_component::init(cx);
            crate::gpui_shell::math_view::register(cx);
            crate::gpui_shell::file_editor::init(cx);
            init(cx);
            windowing::initialize(cx, hub.clone());
        });
        let mut workspace_out = None;
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
            workspace.update(cx, |workspace, cx| {
                // Exercise real workspace tabs without spawning a Shell or
                // altering the machine's saved shortcuts/session.
                workspace.update_keybinds(Vec::new(), cx);
                for path in paths {
                    let view = cx
                        .new(|cx| crate::gpui_shell::code_tab::CodeTabView::new(path, window, cx));
                    let subscription = cx.subscribe(&view, |_, _, _, _| {});
                    workspace.insert_tab_at(
                        workspace.tabs.len(),
                        WorkspaceTab::Code { view, _subscription: subscription },
                        TabMeta::default(),
                    );
                }
                workspace.focus_active(window, cx);
            });
            workspace_out = Some(workspace.clone());
            Root::new(workspace, window, cx)
        });
        window.run_until_parked();
        (directory, workspace_out.unwrap(), window.clone())
    }

    fn assert_active(
        workspace: &Entity<NebulaWorkspace>,
        index: usize,
        cx: &mut VisualTestContext,
    ) {
        cx.update(|window, cx| {
            let workspace = workspace.read(cx);
            assert_eq!(workspace.active, index);
            assert!(!workspace.settings_open);
            let WorkspaceTab::Code { view, .. } = &workspace.tabs[index] else {
                panic!("expected the selected fixture tab");
            };
            assert!(view.read(cx).focus_handle(cx).is_focused(window));
        });
    }

    #[gpui::test]
    fn digits_switch_real_tabs_and_focus_in_both_tab_layouts(cx: &mut TestAppContext) {
        let (_directory, workspace, mut cx) = open_workspace(10, cx);
        for position in
            [nebula_settings::TabsPositionName::Sidebar, nebula_settings::TabsPositionName::Top]
        {
            workspace.update(&mut cx, |workspace, cx| {
                workspace.tabs_position = position;
                workspace.tab_meta[8].has_bell = true;
                cx.notify();
            });
            for modifier in ["ctrl", "alt"] {
                for digit in (1..=9).rev() {
                    press(&format!("{modifier}-{digit}"), &mut cx);
                    assert_active(&workspace, digit - 1, &mut cx);
                }
            }
            assert!(!workspace.read_with(&cx, |workspace, _| workspace.tab_meta[8].has_bell));
        }
    }

    #[gpui::test]
    fn missing_tab_preserves_settings_and_reselecting_a_tab_restores_focus(
        cx: &mut TestAppContext,
    ) {
        let (_directory, workspace, mut cx) = open_workspace(3, cx);
        cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| workspace.open_settings(window, cx));
        });
        press("alt-9", &mut cx);
        press("ctrl-4", &mut cx);
        cx.update(|window, cx| {
            let workspace = workspace.read(cx);
            assert!(workspace.settings_open);
            assert_eq!(workspace.active, 0);
            let settings = &workspace.settings_surface.as_ref().unwrap().0;
            assert!(settings.read(cx).focus_handle(cx).contains_focused(window, cx));
        });
        // The active index was already zero: this still has to leave Settings
        // and return focus to the tab, rather than treating it as a no-op.
        press("ctrl-1", &mut cx);
        assert_active(&workspace, 0, &mut cx);

        cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| workspace.begin_rename(0, window, cx));
        });
        press("alt-1", &mut cx);
        assert_active(&workspace, 0, &mut cx);
    }

    #[gpui::test]
    fn numbered_shortcut_overrides_clear_and_restore_without_restart(cx: &mut TestAppContext) {
        let (_directory, workspace, mut cx) = open_workspace(3, cx);
        workspace.update(&mut cx, |workspace, cx| {
            workspace.update_keybinds(
                vec![
                    ("ctrl+1".into(), "SelectTab3".into()),
                    ("alt+8".into(), "SelectLastTab".into()),
                ],
                cx,
            );
        });
        press("ctrl-1", &mut cx);
        assert_active(&workspace, 2, &mut cx);
        press("ctrl-2", &mut cx);
        press("alt-8", &mut cx);
        assert_active(&workspace, 2, &mut cx);

        workspace.update(&mut cx, |workspace, cx| {
            workspace.update_keybinds(vec![("ctrl+1".into(), "ReceiveChar".into())], cx);
        });
        press("ctrl-1", &mut cx);
        assert_active(&workspace, 2, &mut cx);
        // Removing this override must find the shared display spelling
        // ("Ctrl+1") and restore it despite the stored spelling ("ctrl+1").
        workspace.update(&mut cx, |workspace, cx| workspace.update_keybinds(Vec::new(), cx));
        press("ctrl-1", &mut cx);
        assert_active(&workspace, 0, &mut cx);
    }

    struct TerminalKeyProbe {
        focus: FocusHandle,
        selected: Option<usize>,
        mode: TermMode,
        input: Vec<Keystroke>,
        encoded: Vec<u8>,
    }

    impl Render for TerminalKeyProbe {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .on_action(cx.listener(|this, action: &SelectTab, _, _| {
                    this.selected = action.index_for(10);
                }))
                .child(
                    div()
                        .size_full()
                        .key_context(crate::gpui_shell::terminal::KEY_CONTEXT)
                        .track_focus(&self.focus)
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                            this.input.push(event.keystroke.clone());
                            if let Some(bytes) = crate::gpui_shell::terminal::keymap::encode(
                                &event.keystroke,
                                &this.mode,
                            ) {
                                this.encoded.extend(bytes);
                                cx.stop_propagation();
                            }
                        })),
                )
        }
    }

    #[gpui::test]
    fn numeric_actions_precede_terminal_encoding_and_unbound_digits_pass_through(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            gpui_component::init(cx);
            init(cx);
        });
        let mut probe_out = None;
        let (_, cx) = cx.add_window_view(|window, cx| {
            let probe = cx.new(|cx| TerminalKeyProbe {
                focus: cx.focus_handle(),
                selected: None,
                mode: TermMode::empty(),
                input: Vec::new(),
                encoded: Vec::new(),
            });
            let focus = probe.read(cx).focus.clone();
            focus.focus(window, cx);
            probe_out = Some(probe.clone());
            Root::new(probe, window, cx)
        });
        let probe = probe_out.unwrap();
        for mode in [
            TermMode::empty(),
            TermMode::WIN32_INPUT_MODE,
            TermMode::DISAMBIGUATE_ESC_CODES,
            TermMode::REPORT_ALL_KEYS_AS_ESC,
        ] {
            probe.update(cx, |probe, _| probe.mode = mode);
            for modifier in ["ctrl", "alt"] {
                for digit in 1..=9 {
                    press(&format!("{modifier}-{digit}"), cx);
                    probe.read_with(cx, |probe, _| {
                        assert_eq!(probe.selected, Some(digit - 1));
                        assert!(probe.input.is_empty(), "tab shortcuts must not reach PTY input");
                        assert!(probe.encoded.is_empty());
                    });
                }
            }
        }
        // Plain digits and additional modifiers retain their terminal meaning.
        for combo in ["1", "alt-0", "ctrl-alt-2", "ctrl-shift-2"] {
            press(combo, cx);
            probe.read_with(cx, |probe, _| {
                assert_eq!(probe.selected, Some(8));
                assert_eq!(probe.input.last(), Some(&Keystroke::parse(combo).unwrap()));
            });
        }
        cx.update(|_, cx| {
            cx.bind_keys([workspace_binding_in_context(
                "alt+2",
                &crate::config::Action::ReceiveChar,
                Some(crate::gpui_shell::terminal::KEY_CONTEXT),
            )
            .unwrap()]);
        });
        press("alt-2", cx);
        probe.read_with(cx, |probe, _| {
            assert_eq!(probe.selected, Some(8));
            assert_eq!(probe.input.last(), Some(&Keystroke::parse("alt-2").unwrap()));
            assert!(!probe.encoded.is_empty());
        });
    }
}
