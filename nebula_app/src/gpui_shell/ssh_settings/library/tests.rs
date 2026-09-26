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
            let mut profile = pane.ssh_hosts.profiles.for_destination("nebula-test");
            profile.label = Some("Alpha".into());
            pane.ssh_hosts.profiles.upsert(profile);
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
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.duplicate_ssh_host("nebula-test".into(), window, cx);
            let editor = pane.ssh_editor.as_ref().expect("copied host draft");
            assert!(editor.original_destination.is_none());
            assert!(
                editor
                    .jump_choices
                    .iter()
                    .any(|(host, label)| host == "nebula-test" && label == "Alpha"),
                "the copied-from host remains available as a jump host"
            );
            assert_eq!(pane.ssh_destination_input.read(cx).value(), "");
            assert_eq!(pane.ssh_label_input.read(cx).value(), "Alpha 1");
            pane.close_ssh_editor(window, cx);
        });
    });

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

#[test]
#[ignore = "requires a native Windows desktop and PEBREL_SSH_COPY_QA_DIR"]
fn native_ssh_copy_context_menu_preview() {
    assert_eq!(
        crate::platform::Platform::current(),
        crate::platform::Platform::Windows,
        "this screenshot probe requires Windows",
    );
    use gpui::{Bounds, WindowBounds, WindowOptions, point};
    use std::{
        path::PathBuf,
        sync::{Arc, Mutex},
        time::Duration,
    };

    let output =
        PathBuf::from(std::env::var_os("PEBREL_SSH_COPY_QA_DIR").expect("QA output directory"));
    std::fs::create_dir_all(&output).unwrap();
    let menu_ready = output.join("menu-ready.json");
    assert!(!menu_ready.exists(), "use a fresh QA directory");

    let result = Arc::new(Mutex::new(None));
    let after_run = result.clone();
    gpui_platform::application().with_assets(crate::gpui_shell::assets::NebulaAssets).run(
        move |cx| {
            gpui_component::init(cx);
            cx.set_global(crate::gpui_shell::config::Settings::load(
                nebula_settings::ThemeName::Nord,
            ));
            crate::gpui_shell::theme::apply_chrome_theme(cx);

            let mut pane = None;
            let handle = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                            point(px(60.0), px(70.0)),
                            gpui::size(px(1080.0), px(720.0)),
                        ))),
                        focus: false,
                        show: true,
                        ..Default::default()
                    },
                    |window, cx| {
                        let view = cx.new(|cx| SettingsPane::new(window, cx));
                        view.update(cx, |pane, cx| {
                            pane.manage_launcher_ssh("root@192.0.2.10".into(), false, window, cx);
                            pane.ssh_delete_confirm = None;
                            pane.ssh_hosts = crate::gpui_shell::ssh_hosts::SshHostLists {
                                saved: vec![
                                    "root@192.0.2.10".into(),
                                    "deploy@198.51.100.20".into(),
                                ],
                                ..Default::default()
                            };
                            let mut alpha =
                                pane.ssh_hosts.profiles.for_destination("root@192.0.2.10");
                            alpha.label = Some("Alpha".into());
                            alpha.auth = crate::ssh_profiles::SshAuthMode::PublicKey;
                            alpha.private_keys.push("C:\\Keys\\alpha_ed25519".into());
                            pane.ssh_hosts.profiles.upsert(alpha);
                            let mut beta =
                                pane.ssh_hosts.profiles.for_destination("deploy@198.51.100.20");
                            beta.label = Some("Beta".into());
                            pane.ssh_hosts.profiles.upsert(beta);
                        });
                        pane = Some(view.clone());
                        cx.new(|cx| gpui_component::Root::new(view, window, cx))
                    },
                )
                .unwrap();
            let pane = pane.unwrap();

            cx.spawn(async move |cx| {
                cx.background_executor().timer(Duration::from_millis(700)).await;
                let opened = cx
                    .update_window(handle.into(), |_, window, cx| -> Result<(), String> {
                        let _ = window.draw(cx);
                        Ok(())
                    })
                    .map_err(|error| error.to_string())
                    .and_then(|result| result);

                if opened.is_ok() {
                    std::fs::write(
                        &menu_ready,
                        serde_json::to_vec(&serde_json::json!({
                            "pid": std::process::id(),
                            "state": "host-row-ready",
                            "host": "Alpha",
                        }))
                        .unwrap(),
                    )
                    .unwrap();
                    for _ in 0..150 {
                        if output.join("capture-complete").exists() {
                            break;
                        }
                        cx.background_executor().timer(Duration::from_millis(200)).await;
                    }
                }
                *result.lock().unwrap() = Some(opened);

                let _ = cx.update_window(handle.into(), |_, window, cx| {
                    pane.update(cx, |pane, cx| pane.close_ssh_editor(window, cx));
                    let _ = window.draw(cx);
                    window.dispatch_event(
                        gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                            position: point(px(300.0), px(220.0)),
                            pressed_button: None,
                            modifiers: gpui::Modifiers::default(),
                        }),
                        cx,
                    );
                    window.dispatch_event(
                        gpui::PlatformInput::MouseDown(gpui::MouseDownEvent {
                            position: point(px(300.0), px(220.0)),
                            button: gpui::MouseButton::Right,
                            modifiers: gpui::Modifiers::default(),
                            click_count: 1,
                            first_mouse: false,
                        }),
                        cx,
                    );
                    window.remove_window();
                });
                drop(pane);
                cx.update(|cx| cx.quit());
            })
            .detach();
        },
    );
    assert_eq!(*after_run.lock().unwrap(), Some(Ok(())));
}
