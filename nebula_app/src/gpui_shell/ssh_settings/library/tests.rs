use super::*;

#[test]
fn pairing_design_ssh_auth_description_uses_metadata_not_a_saved_password_claim() {
    let profiles = crate::ssh_profiles::SshProfiles::default();
    let mut profile = profiles.for_destination("example");
    let language = crate::display::UiLanguage::EnUs;
    assert_eq!(host_auth_label(&profile, language), "Automatic");
    profile.auth = crate::ssh_profiles::SshAuthMode::Password;
    assert_eq!(host_auth_label(&profile, language), "Password");
    profile.auth = crate::ssh_profiles::SshAuthMode::PublicKey;
    profile.private_keys.push(std::path::PathBuf::from("private/location/id_ed25519"));
    assert_eq!(host_auth_label(&profile, language), "id_ed25519");
}

#[cfg(feature = "gpui-test-support")]
#[gpui::test]
fn pairing_design_ssh_cards_keep_icon_anchors_and_compact_filter(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_global(crate::gpui_shell::config::Settings::load(nebula_settings::ThemeName::Nord));
    });
    let mut pane = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| SettingsPane::new(window, cx));
        view.update(cx, |pane, cx| {
            pane.manage_launcher_ssh(String::new(), false, window, cx);
            pane.ssh_delete_confirm = None;
            pane.ssh_hosts = crate::gpui_shell::ssh_hosts::SshHostLists {
                saved: vec!["nebula-test".into(), "second-host".into()],
                pinned: vec!["second-host".into()],
                ..Default::default()
            };
        });
        pane = Some(view.clone());
        gpui_component::Root::new(view, window, cx)
    });
    let pane = pane.unwrap();
    for width in [1280.0, 800.0] {
        cx.simulate_resize(gpui::size(px(width), px(1100.0)));
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        let icon = cx.debug_bounds("ssh-host-icon-0").expect("card anchor");
        assert_eq!(icon.size, gpui::size(px(36.0), px(36.0)));
        let first = cx.debug_bounds("ssh-host-row-0").unwrap();
        assert_eq!(icon.center().y, first.center().y);
        let next = cx.debug_bounds("ssh-host-row-1").unwrap();
        let next_icon = cx.debug_bounds("ssh-host-icon-1").unwrap();
        assert_eq!(icon.center().x, next_icon.center().x);
        assert!(next.origin.y - first.bottom() >= px(8.0));
        let filter = cx.debug_bounds("ssh-inline-filter").unwrap();
        assert!(filter.size.width <= px(210.0));
        assert!(filter.right() <= px(width));
        for name in ["ssh-edit-0", "ssh-pin-0", "ssh-delete-0"] {
            let action = cx.debug_bounds(name).expect("visible management action");
            assert!(action.size.width >= px(32.0) && action.size.height >= px(32.0));
            assert!(action.right() <= first.right());
        }
    }
    let pin_filter = cx.debug_bounds("host-scope-1").unwrap();
    let point = gpui::point(pin_filter.origin.x + px(4.0), pin_filter.center().y);
    cx.simulate_mouse_down(point, MouseButton::Left, gpui::Modifiers::default());
    cx.simulate_mouse_up(point, MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.update(|_, cx| {
        pane.update(cx, |pane, cx| {
            assert!(pane.ssh_library.scope == HostScope::Pinned);
            assert_eq!(pane.filtered_library_hosts(cx), vec!["second-host"]);
            pane.ssh_hosts.hidden.push("second-host".into());
            assert!(pane.filtered_library_hosts(cx).is_empty());
        })
    });
}
