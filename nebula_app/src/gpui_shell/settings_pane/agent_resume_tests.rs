use super::*;
use crate::ai_agents::AgentKind;
use crate::ai_hook::integrations::{AgentIntegration, HookInspection};
use crate::gpui_shell::settings_fixture::{SettingsBytesGuard, lock_theme_studio};
use gpui::{TestAppContext, size};

#[gpui::test]
fn agent_resume_rules_persist_toggle_and_remove_through_settings_ui(cx: &mut TestAppContext) {
    let _lock = lock_theme_studio();
    let _settings = SettingsBytesGuard::capture();
    std::fs::write(nebula_settings::settings_path(), "").unwrap();
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_reduce_motion(true);
        cx.set_global(crate::gpui_shell::config::Settings::load(ThemeName::Nord));
    });
    let mut pane = None;
    let (_, window) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| SettingsPane::new(window, cx));
        view.update(cx, |pane, _| {
            pane.active_section = 10;
            pane.agents.rows = Some(vec![AgentIntegration {
                agent: AgentKind::Claude,
                executable: None,
                hook: Some(nebula_settings::AgentHook::Claude),
                inspection: HookInspection::default(),
            }]);
        });
        pane = Some(view.clone());
        gpui_component::Root::new(view, window, cx)
    });
    let pane = pane.unwrap();
    window.simulate_resize(size(px(1040.0), px(900.0)));
    window.run_until_parked();
    window.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });

    assert!(window.debug_bounds("agent-resume-picker").is_some());
    assert!(window.debug_bounds("agent-resume-input").is_some());
    assert!(window.debug_bounds("agent-resume-empty").is_some());

    // Reject shell injection / unquoted operator in rule draft.
    window.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.select_agent_resume_rule(AgentKind::Codex, window, cx);
            let input = pane.agents.resume_ui.as_ref().unwrap().args_input.clone();
            input.update(cx, |input, cx| input.set_value("--yolo; echo bad", window, cx));
            pane.save_agent_resume_rule(window, cx);
        });
    });
    window.run_until_parked();
    assert_eq!(nebula_settings::RuntimeSettings::load().agent_resume_args.get("codex"), "");

    // Save a valid rule for Codex (--yolo) and Antigravity (--dangerously-skip-permissions).
    window.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            let input = pane.agents.resume_ui.as_ref().unwrap().args_input.clone();
            input.update(cx, |input, cx| input.set_value("--yolo", window, cx));
            pane.save_agent_resume_rule(window, cx);

            pane.select_agent_resume_rule(AgentKind::Antigravity, window, cx);
            let input = pane.agents.resume_ui.as_ref().unwrap().args_input.clone();
            input.update(cx, |input, cx| {
                input.set_value("--dangerously-skip-permissions", window, cx);
            });
            pane.save_agent_resume_rule(window, cx);
        });
    });
    window.run_until_parked();
    window.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });

    assert!(window.debug_bounds("agent-resume-rule-codex").is_some());
    assert!(window.debug_bounds("agent-resume-rule-antigravity").is_some());
    assert!(window.debug_bounds("agent-resume-cmd-warning").is_none());
    assert_eq!(nebula_settings::RuntimeSettings::load().agent_resume_args.get("codex"), "--yolo");
    assert_eq!(
        nebula_settings::RuntimeSettings::load().agent_resume_args.get("antigravity"),
        "--dangerously-skip-permissions"
    );

    // Saving a valid rule containing `"`, `%`, or `!` persists for POSIX/PowerShell and warns
    // explicitly that `cmd.exe` will reject it.
    window.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.select_agent_resume_rule(AgentKind::Codex, window, cx);
            let input = pane.agents.resume_ui.as_ref().unwrap().args_input.clone();
            input.update(cx, |input, cx| {
                input.set_value(r#"["--config","model_provider=\"custom\""]"#, window, cx);
            });
            pane.save_agent_resume_rule(window, cx);
        });
        window.refresh();
        window.draw(cx).clear(cx);
    });
    assert!(window.debug_bounds("agent-resume-cmd-warning").is_some());
    assert_eq!(
        nebula_settings::RuntimeSettings::load().agent_resume_args.get("codex"),
        r#"["--config","model_provider=\"custom\""]"#
    );

    // Toggle Codex rule off -> remains in rules list, but `get` returns empty.
    window.update(|_, cx| {
        pane.update(cx, |pane, cx| {
            pane.toggle_agent_resume_rule(AgentKind::Codex, false, cx);
        });
    });
    window.run_until_parked();
    assert_eq!(nebula_settings::RuntimeSettings::load().agent_resume_args.get("codex"), "");
    assert_eq!(
        nebula_settings::RuntimeSettings::load()
            .agent_resume_args
            .rule("codex")
            .map(|rule| rule.enabled),
        Some(false)
    );

    // Remove Codex rule -> removed from persisted rules.
    window.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.remove_agent_resume_rule(AgentKind::Codex, window, cx);
        });
    });
    window.run_until_parked();
    assert!(nebula_settings::RuntimeSettings::load().agent_resume_args.rule("codex").is_none());
    assert_eq!(
        nebula_settings::RuntimeSettings::load().agent_resume_args.get("antigravity"),
        "--dangerously-skip-permissions"
    );
}
