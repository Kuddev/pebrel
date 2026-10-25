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
    assert_eq!(host_auth_label(&profile, language), "Private key id_ed25519");
}

#[cfg(feature = "gpui-test-support")]
#[gpui::test]
fn prototype_ssh_rows_keep_icon_anchors_and_compact_filter(cx: &mut gpui::TestAppContext) {
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
                configured: vec!["hidden-config".into()],
                hidden: vec!["hidden-config".into()],
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
        assert_eq!(icon.size, gpui::size(px(32.0), px(32.0)));
        let first = cx.debug_bounds("ssh-host-row-0").unwrap();
        assert!(cx.debug_bounds("ssh-host-time-0").is_none(), "unknown time column is removed");
        // A one-pixel bottom divider shifts the content center by half a pixel.
        assert!((f32::from(icon.center().y - first.center().y)).abs() <= 0.5);
        let next = cx.debug_bounds("ssh-host-row-1").unwrap();
        let next_icon = cx.debug_bounds("ssh-host-icon-1").unwrap();
        assert_eq!(icon.center().x, next_icon.center().x);
        assert_eq!(next.origin.y, first.bottom(), "rows share one panel without card gutters");
        let filter = cx.debug_bounds("ssh-inline-filter").unwrap();
        assert!(filter.size.width <= px(240.0));
        assert!(filter.size.height >= px(32.0), "single-line search keeps the toolbar height");
        assert!(filter.right() <= px(width));
        let controls = cx.debug_bounds("ssh-library-controls").unwrap();
        assert!(controls.bottom() <= first.top(), "filters stay above the list panel");
        let footer = cx.debug_bounds("ssh-config-banner").unwrap();
        assert!(footer.top() >= next.bottom());
        assert!(cx.debug_bounds("host-scope-2").is_some());
        assert!(cx.debug_bounds("host-scope-3").is_none());
        assert!(cx.debug_bounds("ssh-toggle-hidden").is_some());
        for name in ["ssh-more-0", "ssh-connect-0"] {
            let action = cx.debug_bounds(name).expect("visible management action");
            assert!(action.size.width >= px(32.0) && action.size.height >= px(32.0));
            assert!(action.right() <= first.right());
        }
    }
    let more = cx.debug_bounds("ssh-more-1").unwrap();
    let connect_before = cx.debug_bounds("ssh-connect-1").unwrap();
    let hovered_row = cx.debug_bounds("ssh-host-row-1").unwrap();
    cx.simulate_mouse_move(hovered_row.center(), None, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    assert_eq!(cx.debug_bounds("ssh-connect-1").unwrap(), connect_before);
    cx.simulate_click(more.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    // Move away from the row into the menu area; the open dropdown owns its
    // visibility and keyboard navigation must continue to work.
    cx.simulate_mouse_move(gpui::point(px(10.0), px(10.0)), None, gpui::Modifiers::default());
    cx.run_until_parked();
    // 复制失败也不能退回编辑弹窗；用明确的加载错误阻止测试触碰用户凭据和磁盘。
    pane.update(cx, |pane, _| pane.ssh_hosts.load_error = Some("fixture load failure".into()));
    cx.simulate_keystrokes("down down down enter");
    cx.run_until_parked();
    cx.update(|_, cx| {
        pane.update(cx, |pane, cx| {
            assert!(pane.ssh_editor.is_none(), "copy does not open an editor");
            assert_eq!(pane.ssh_hosts.profiles.destinations().count(), 1);
            assert!(matches!(&pane.ssh_status, Some(SshStatus::Error(error)) if error == "fixture load failure"));
            pane.ssh_hosts.load_error = None;
            cx.notify();
        });
    });

    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    let row = cx.debug_bounds("ssh-host-row-0").unwrap();
    cx.simulate_mouse_down(row.center(), MouseButton::Right, gpui::Modifiers::default());
    cx.simulate_mouse_up(row.center(), MouseButton::Right, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_keystrokes("down down down down down enter");
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    pane.read_with(cx, |pane, _| {
        assert_eq!(pane.ssh_delete_confirm.as_deref(), Some("second-host"));
        assert_eq!(pane.ssh_hosts.saved.len(), 2, "menu only requests confirmation");
        assert!(pane.ssh_delete_undo.is_none());
    });
    let cancel = cx.debug_bounds("ssh-cancel-delete-0").unwrap();
    cx.simulate_click(cancel.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    assert!(pane.read_with(cx, |pane, _| pane.ssh_delete_confirm.is_none()));
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    let hidden = cx.debug_bounds("ssh-toggle-hidden").unwrap();
    cx.simulate_click(hidden.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    assert!(cx.debug_bounds("ssh-hidden-row-0").is_some());
    assert!(pane.read_with(cx, |pane, _| pane.ssh_show_hidden));

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
            let theme = if std::env::var("PEBREL_SSH_QA_THEME").as_deref() == Ok("paper") {
                nebula_settings::ThemeName::Paper
            } else {
                nebula_settings::ThemeName::Nord
            };
            let mut settings = crate::gpui_shell::config::Settings::load(theme);
            settings.ui_language = crate::display::UiLanguage::ZhCn;
            cx.set_global(settings);
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
                            pane.manage_launcher_ssh(String::new(), false, window, cx);
                            pane.ssh_delete_confirm = None;
                            let fixtures = [
                                ("kud@nas.example", "家里 NAS", "家里"),
                                ("deploy@192.0.2.21", "生产 API", "公司"),
                                ("bastion", "bastion", "公司"),
                                ("kud@relay.example", "中转服务器", ""),
                                ("gpu-box", "gpu-box", "家里"),
                                ("pi@raspberrypi.example", "raspberrypi", "家里"),
                            ];
                            pane.ssh_hosts = crate::gpui_shell::ssh_hosts::SshHostLists {
                                saved: fixtures.iter().map(|(host, _, _)| (*host).into()).collect(),
                                pinned: fixtures[..2]
                                    .iter()
                                    .map(|(host, _, _)| (*host).into())
                                    .collect(),
                                configured: vec![
                                    "bastion".into(),
                                    "gpu-box".into(),
                                    "old-vps".into(),
                                ],
                                hidden: vec!["old-vps".into()],
                                ..Default::default()
                            };
                            for (index, (host, label, group)) in fixtures.into_iter().enumerate() {
                                let mut profile = pane.ssh_hosts.profiles.for_destination(host);
                                profile.label = Some(label.into());
                                if matches!(index, 0 | 3) {
                                    profile.auth = crate::ssh_profiles::SshAuthMode::PublicKey;
                                    profile.private_keys.push("C:\\Keys\\id_ed25519".into());
                                }
                                if index == 1 {
                                    profile.connection.jump_mode =
                                        crate::ssh_profiles::SshHostJumpMode::Host;
                                    profile.connection.jump_host = "bastion".into();
                                }
                                pane.ssh_hosts.profiles.upsert(profile);
                                pane.ssh_hosts
                                    .profiles
                                    .set_organization(
                                        host,
                                        crate::ssh_profiles::HostOrganization {
                                            group: group.into(),
                                            ..Default::default()
                                        },
                                    )
                                    .unwrap();
                            }
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
                            "host": "家里 NAS",
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
