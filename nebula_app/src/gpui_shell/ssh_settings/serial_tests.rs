use super::*;
use crate::gpui_shell::settings_fixture::{SettingsBytesGuard, lock_theme_studio};
use gpui::{AppContext as _, Focusable as _};

struct ProfilesGuard(std::path::PathBuf, Option<Vec<u8>>);
impl Drop for ProfilesGuard {
    fn drop(&mut self) {
        if let Some(bytes) = &self.1 {
            std::fs::write(&self.0, bytes).unwrap();
        } else {
            let _ = std::fs::remove_file(&self.0);
        }
    }
}

#[gpui::test]
fn serial_editor_saves_reloads_and_reopens_without_ssh_username(cx: &mut gpui::TestAppContext) {
    let _lock = lock_theme_studio();
    let _settings = SettingsBytesGuard::capture();
    let profile_path = crate::display::nebula_data_dir().join("ssh_profiles.json");
    let _profiles = ProfilesGuard(profile_path.clone(), std::fs::read(&profile_path).ok());
    nebula_settings::persist_keys(&[
        ("saved_hosts", "".into()),
        ("pinned_hosts", "".into()),
        ("hidden_hosts", "".into()),
    ])
    .unwrap();
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_global(crate::gpui_shell::config::Settings::load(nebula_settings::ThemeName::Nord));
    });
    let mut pane_out = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let pane = cx.new(|cx| SettingsPane::new(window, cx));
        pane_out = Some(pane.clone());
        gpui_component::Root::new(pane, window, cx)
    });
    let pane = pane_out.unwrap();
    cx.simulate_resize(gpui::size(px(1200.0), px(1000.0)));
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.open_ssh_editor(None, window, cx);
            assert_eq!(pane.ssh_username_input.read(cx).value().as_str(), "root");
            pane.ssh_destination_input.read(cx).focus_handle(cx).focus(window, cx);
        })
    });
    let destination = "serial:COM3,115200,8N1";
    cx.simulate_input(destination);
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let save = cx.debug_bounds("ssh-editor-save").expect("visible save button");
    cx.simulate_click(save.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    pane.read_with(cx, |pane, _| {
        assert!(pane.ssh_editor.is_none(), "valid serial address must save and close editor");
        assert!(matches!(&pane.ssh_status, Some(SshStatus::Saved(saved)) if saved == destination));
    });
    let mut hosts = crate::gpui_shell::ssh_hosts::SshHostLists::load();
    assert_eq!(hosts.saved, vec![destination]);
    assert!(hosts.profiles.contains(destination));
    hosts.toggle_pin(destination);
    hosts.persist().unwrap();
    assert!(crate::gpui_shell::ssh_hosts::SshHostLists::load().is_pinned(destination));
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.open_ssh_editor(Some(destination.into()), window, cx);
            assert_eq!(pane.ssh_destination_from_draft(cx).unwrap(), destination);
            assert!(pane.ssh_connection_from_draft(destination, cx).is_ok());
            pane.ssh_destination_input
                .update(cx, |input, cx| input.set_value("serial:COM3,0,8N1", window, cx));
            pane.save_ssh_editor(window, cx);
            assert!(pane.ssh_editor.is_some(), "invalid baud must keep the editor open");
        })
    });
    assert_eq!(crate::gpui_shell::ssh_hosts::SshHostLists::load().saved, vec![destination]);
}
