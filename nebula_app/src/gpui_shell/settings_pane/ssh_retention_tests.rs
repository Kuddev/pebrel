use super::*;
use gpui_component::Root;

#[test]
fn isolated_ssh_retention_settings_control() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join(".ssh")).unwrap();
    std::fs::write(directory.path().join(".ssh/config"), "").unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "gpui_shell::settings_pane::ssh_retention_tests::ended_ssh_switch_persists_and_failed_save_keeps_the_previous_value",
            "--ignored",
            "--nocapture",
        ])
        .env_remove("PEBREL_CONFIG_FILE")
        .env_remove("NEBULA_CONFIG_FILE")
        .env_remove("PEBREL_GPUI_CONFIG")
        .env_remove("NEBULA_GPUI_CONFIG")
        .env_remove("SSH_AUTH_SOCK")
        .env_remove("NEBULA_GPUI_OPEN_DOC")
        .env("PEBREL_TEST_SSH_HOME", directory.path())
        .env("PEBREL_CONFIG_DIR", directory.path())
        .env("PEBREL_SSH_LIFECYCLE_QA_DIR", directory.path())
        .output()
        .unwrap();
    let evidence = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success() && evidence.contains("1 passed;"), "{evidence}");
}

struct KeyboardRetentionControl {
    pane: Entity<SettingsPane>,
    focus: FocusHandle,
}

impl gpui::Render for KeyboardRetentionControl {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let checked = self.pane.read(cx).runtime.keep_ssh_session;
        let pane = self.pane.clone();
        div().track_focus(&self.focus).child(
            crate::gpui_shell::widgets::NebulaSwitch::new("keep_ssh_session")
                .checked(checked)
                .on_click(cx.listener(move |_, checked, window, cx| {
                    pane.update(cx, |pane, cx| {
                        pane.toggle("keep_ssh_session", *checked, window, cx)
                    });
                    cx.notify();
                })),
        )
    }
}

#[gpui::test]
#[ignore = "launched by isolated_ssh_retention_settings_control with synthetic preferences"]
fn ended_ssh_switch_persists_and_failed_save_keeps_the_previous_value(
    cx: &mut gpui::TestAppContext,
) {
    let directory = std::path::PathBuf::from(
        std::env::var_os("PEBREL_SSH_LIFECYCLE_QA_DIR").expect("isolated fixture directory"),
    );
    assert_eq!(Some(directory.as_os_str()), std::env::var_os("PEBREL_CONFIG_DIR").as_deref());
    let path = directory.join("pebrel_settings.txt");
    assert!(!path.exists());
    std::fs::write(
        &path,
        "language=en-US\nai_hooks=0\nauto_check_updates=0\nkeep_session=1\nprivate_setting=keep\n",
    )
    .unwrap();
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_global(crate::gpui_shell::config::Settings::load(ThemeName::Nord));
        cx.set_reduce_motion(true);
    });
    let mut pane = None;
    let (_, window) = cx.add_window_view(|window, cx| {
        let entity = cx.new(|cx| SettingsPane::new(window, cx));
        entity.update(cx, |pane, _| pane.active_section = 8);
        pane = Some(entity.clone());
        Root::new(entity, window, cx)
    });
    let pane = pane.unwrap();
    window.simulate_resize(gpui::size(px(1100.0), px(1000.0)));
    window.run_until_parked();
    window.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert!(!pane.read_with(window, |pane, _| pane.runtime.keep_ssh_session));
    assert!(RuntimeSettings::load().keep_session);
    let bounds = window.debug_bounds("nebula-switch-keep_ssh_session").unwrap();
    assert!(bounds.size.height >= px(22.0));
    assert!(bounds.origin.y >= px(0.0) && bounds.bottom() <= px(1000.0));
    window.simulate_mouse_move(bounds.center(), None, gpui::Modifiers::default());
    window.simulate_click(bounds.center(), gpui::Modifiers::default());
    window.run_until_parked();
    assert!(RuntimeSettings::load().keep_ssh_session);
    pane.read_with(window, |pane, cx| {
        assert!(pane.runtime.keep_ssh_session);
        assert!(cx.global::<crate::gpui_shell::config::Settings>().keep_ssh_session);
        assert_eq!(pane.setting_override("keep_ssh_session"), Some((true, "0".into())));
    });
    // Button deliberately preserves focus on mouse down. Exercise the same
    // production switch and settings toggle through a real Tab stop, without
    // assuming that the preceding pointer click focused the switch.
    window.update(|window, cx| {
        let control =
            cx.new(|cx| KeyboardRetentionControl { pane: pane.clone(), focus: cx.focus_handle() });
        window.replace_root(cx, |window, cx| Root::new(control.clone(), window, cx));
        let focus = control.read(cx).focus.clone();
        focus.focus(window, cx);
        let _ = window.draw(cx);
    });
    window.simulate_keystrokes("tab enter");
    window.simulate_event(gpui::KeyUpEvent { keystroke: gpui::Keystroke::parse("enter").unwrap() });
    window.run_until_parked();
    assert!(!RuntimeSettings::load().keep_ssh_session);
    window.simulate_keystrokes("enter");
    window.simulate_event(gpui::KeyUpEvent { keystroke: gpui::Keystroke::parse("enter").unwrap() });
    window.run_until_parked();
    assert!(RuntimeSettings::load().keep_ssh_session);
    window.update(|window, cx| {
        window.replace_root(cx, |window, cx| Root::new(pane.clone(), window, cx));
        let _ = window.draw(cx);
    });
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(saved.contains("private_setting=keep\n") && saved.contains("keep_session=1\n"));

    // A directory at the synthetic settings path deterministically rejects persistence.
    let backup = directory.join("before-save-error.txt");
    std::fs::rename(&path, &backup).unwrap();
    std::fs::create_dir(&path).unwrap();
    window.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let bounds = window.debug_bounds("nebula-switch-keep_ssh_session").unwrap();
    window.simulate_click(bounds.center(), gpui::Modifiers::default());
    window.run_until_parked();
    assert!(pane.read_with(window, |pane, _| pane.runtime.keep_ssh_session));
    window.update(|_, cx| {
        assert!(cx.global::<crate::gpui_shell::config::Settings>().keep_ssh_session);
    });
    std::fs::remove_dir(&path).unwrap();
    std::fs::rename(backup, &path).unwrap();
}
