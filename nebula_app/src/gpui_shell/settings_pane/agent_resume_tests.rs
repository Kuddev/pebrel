use super::*;
use crate::gpui_shell::settings_fixture::{SettingsBytesGuard, lock_theme_studio};

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
}

fn click(selector: &'static str, cx: &mut VisualTestContext) {
    let bounds = cx.debug_bounds(selector).expect("visible control");
    cx.simulate_click(bounds.center(), Modifiers::default());
    draw(cx);
}

fn edit_input(selector: &'static str, value: &str, cx: &mut VisualTestContext) {
    click(selector, cx);
    cx.simulate_keystrokes(if cfg!(target_os = "macos") { "cmd-a" } else { "ctrl-a" });
    cx.simulate_input(value);
    draw(cx);
}

#[gpui::test]
fn resume_arguments_save_reopen_reject_invalid_and_clear_through_real_controls(
    cx: &mut TestAppContext,
) {
    let _fixture_guard = lock_theme_studio();
    let _settings_guard = SettingsBytesGuard::capture();
    std::fs::create_dir_all(nebula_settings::settings_dir()).unwrap();
    std::fs::write(nebula_settings::settings_path(), "theme=Nord\nlanguage=en-US\n").unwrap();
    let (pane, window, reply, _) = fixture(cx);
    drop(reply);
    draw(window);
    let value = r#"["--yolo", "--config", "model=example"]"#;
    edit_input("agent-resume-input-1", value, window);
    assert_eq!(RuntimeSettings::load().agent_resume_args.get("codex"), "");
    click("agent-resume-save-1", window);
    let persisted = r#"["--yolo","--config","model=example"]"#;
    assert_eq!(RuntimeSettings::load().agent_resume_args.get("codex"), persisted);
    window.read(|cx| {
        assert_eq!(
            cx.global::<crate::gpui_shell::config::Settings>().agent_resume_args.get("codex"),
            persisted
        );
    });
    pane.read_with(window, |pane, _| {
        assert!(matches!(pane.agents.resume_feedback, Some((1, Ok(())))));
    });
    let reopened = window.update(|window, cx| cx.new(|cx| SettingsPane::new(window, cx)));
    reopened.read_with(window, |pane, cx| {
        assert_eq!(pane.agents.resume_inputs[1].read(cx).value().as_ref(), persisted);
        assert_eq!(pane.runtime.agent_resume_args.get("claude"), "");
    });
    edit_input("agent-resume-input-1", "--yolo; echo invalid", window);
    click("agent-resume-save-1", window);
    assert_eq!(RuntimeSettings::load().agent_resume_args.get("codex"), persisted);
    pane.read_with(window, |pane, _| {
        assert!(matches!(pane.agents.resume_feedback, Some((1, Err(_)))));
    });
    edit_input("agent-resume-input-1", "[]", window);
    click("agent-resume-save-1", window);
    assert_eq!(RuntimeSettings::load().agent_resume_args.get("codex"), "");
    let input = window.debug_bounds("agent-resume-input-1").unwrap();
    let save = window.debug_bounds("agent-resume-save-1").unwrap();
    assert!(input.right() <= save.left());
    assert!(save.size.height >= px(24.0));
}
