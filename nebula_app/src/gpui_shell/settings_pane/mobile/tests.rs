use super::*;
use crate::gpui_shell::copy_feedback::COPY_FEEDBACK_TTL;
use crate::mobile_connection::{ConnectionSnapshot, DeviceSummary};

fn fixture(enabled: bool, paired: bool) -> Snapshot {
    let invitation = serde_json::json!({
        "fixture": "complete invitation", "secure": { "expiresAt": now() + 600 }
    })
    .to_string();
    Snapshot {
        preferences: Preferences { enabled, ..Default::default() },
        lan: enabled.then_some(ConnectionSnapshot {
            status: Status::Waiting,
            invitation: Some(invitation),
            address: "wss://192.0.2.1:4567".into(),
            pairing_code: Some("48271936".into()),
            discoverable: true,
        }),
        relay: None,
        devices: if paired {
            vec![DeviceSummary {
                id: "fixture-phone".into(),
                name: "Fixture phone".into(),
                allow_input: false,
                connected: true,
                route: Some(Mode::Lan),
            }]
        } else {
            Vec::new()
        },
        requests: Vec::new(),
    }
}

fn fixture_addresses() -> Vec<connection::LanAddress> {
    [("Ethernet", "192.0.2.1"), ("Tailscale", "100.64.0.8")]
        .into_iter()
        .map(|(name, address)| connection::LanAddress {
            address: address.parse().unwrap(),
            name: name.into(),
            preferred: name == "Ethernet",
        })
        .collect()
}

#[gpui::test]
fn refreshing_interfaces_never_selects_a_replacement_for_saved_tailscale(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_global(crate::gpui_shell::config::Settings::load(ThemeName::Nord));
    });
    let mut owner = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let pane = cx.new(|cx| SettingsPane::new(window, cx));
        pane.update(cx, |pane, cx| {
            pane.mobile.initialized = true;
            let mut snapshot = fixture(true, true);
            snapshot.preferences.address = Some("100.64.0.8".parse().unwrap());
            pane.mobile.display_snapshot(snapshot);
            pane.mobile_set_addresses(fixture_addresses(), window, cx);
        });
        owner = Some(pane.clone());
        gpui_component::Root::new(pane, window, cx)
    });
    let pane = owner.unwrap();
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            let selected = Some("100.64.0.8".parse().unwrap());
            assert_eq!(pane.mobile_selected_address(cx), selected);
            for available in [vec![fixture_addresses().remove(0)], Vec::new()] {
                pane.mobile_set_addresses(available, window, cx);
                assert_eq!(pane.mobile_selected_address(cx), None);
                assert_eq!(pane.mobile.preferences().address, selected);
            }
            pane.mobile_set_addresses(fixture_addresses(), window, cx);
            assert_eq!(pane.mobile_selected_address(cx), selected);
        });
    });
}

#[gpui::test]
fn failed_network_change_restores_the_committed_interface(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_global(crate::gpui_shell::config::Settings::load(ThemeName::Nord));
    });
    let mut owner = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let pane = cx.new(|cx| SettingsPane::new(window, cx));
        pane.update(cx, |pane, cx| {
            pane.mobile.initialized = true;
            let mut snapshot = fixture(true, false);
            snapshot.preferences.address = Some("192.0.2.1".parse().unwrap());
            pane.mobile.display_snapshot(snapshot);
            pane.mobile_set_addresses(fixture_addresses(), window, cx);
            pane.mobile.address_select.update(cx, |select, cx| {
                select.set_selected_index(Some(IndexPath::default().row(1)), window, cx);
            });
            assert_eq!(pane.mobile_selected_address(cx), Some("100.64.0.8".parse().unwrap()));
            pane.mobile_run(None, false, || Err(Failure::Connection), window, cx);
        });
        owner = Some(pane.clone());
        gpui_component::Root::new(pane, window, cx)
    });
    let pane = owner.unwrap();
    cx.run_until_parked();
    pane.read_with(cx, |pane, cx| {
        assert_eq!(pane.mobile.failure, Some(Failure::Connection));
        assert!(!pane.mobile.operation);
        assert_eq!(pane.mobile_selected_address(cx), Some("192.0.2.1".parse().unwrap()));
        assert_eq!(pane.mobile_selected_address(cx), pane.mobile.preferences().address);
    });
}

#[gpui::test]
fn mobile_three_states_and_manual_copy_use_the_rendered_controls(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_global(crate::gpui_shell::config::Settings::load(ThemeName::Nord));
    });
    let mut owner = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let pane = cx.new(|cx| SettingsPane::new(window, cx));
        pane.update(cx, |pane, cx| {
            pane.active_section = 11;
            // 布局夹具不启动真实监听，也不读取或修改用户的手机授权。
            pane.mobile.initialized = true;
            pane.mobile.display_snapshot(fixture(false, false));
            pane.mobile_set_addresses(fixture_addresses(), window, cx);
        });
        owner = Some(pane.clone());
        gpui_component::Root::new(pane, window, cx)
    });
    let pane = owner.unwrap();
    cx.simulate_resize(gpui::size(px(1280.0), px(1000.0)));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let off = cx.debug_bounds("mobile-off").expect("off state has actual layout");
    let enable = cx.debug_bounds("mobile-enable").expect("enable action is rendered");
    assert!(enable.size.width < off.size.width / 2.0, "enable action keeps its content width");
    assert!(cx.debug_bounds("mobile-pairing").is_none());
    assert!(cx.debug_bounds("mobile-interface-row").is_none());

    cx.update(|_, cx| {
        pane.update(cx, |pane, cx| {
            pane.mobile.display_snapshot(fixture(true, false));
            cx.notify();
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert!(cx.debug_bounds("mobile-off").is_none());
    assert!(cx.debug_bounds("mobile-pairing").is_some());
    assert!(cx.debug_bounds("mobile-short-code").is_some());
    let network = cx
        .debug_bounds("mobile-interface-row")
        .expect("network selection is available before the first pairing");
    let port =
        cx.debug_bounds("mobile-port-row").expect("port is editable before the first pairing");
    let pairing = cx.debug_bounds("mobile-pairing").unwrap();
    let modes = cx.debug_bounds("mobile-pairing-modes").unwrap();
    let steps = cx.debug_bounds("mobile-pairing-steps").unwrap();
    assert!(
        pairing.origin.y <= modes.origin.y
            && modes.bottom() <= network.origin.y
            && network.bottom() <= port.origin.y
            && port.bottom() <= steps.origin.y
            && steps.bottom() <= pairing.bottom(),
        "endpoint settings stay inside the pairing card, after the connection type and before the steps"
    );
    let qr = cx.debug_bounds("mobile-qr").expect("QR has actual layout");
    let copy =
        cx.debug_bounds("mobile-copy-invitation").expect("manual pairing action is rendered");
    assert!(copy.size.height >= px(28.0));
    cx.simulate_click(copy.center(), gpui::Modifiers::default());
    let expected = pane.read_with(cx, |pane, _| pane.mobile.qr_payload.clone().unwrap());
    assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), expected);
    assert!(pane.read_with(cx, |pane, cx| pane.mobile.copy_feedback.read(cx).is_copied()));
    cx.run_until_parked();
    cx.executor().advance_clock(COPY_FEEDBACK_TTL / 2);
    cx.simulate_click(copy.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    cx.executor().advance_clock(COPY_FEEDBACK_TTL / 2);
    cx.run_until_parked();
    assert!(pane.read_with(cx, |pane, cx| pane.mobile.copy_feedback.read(cx).is_copied()));
    cx.executor().advance_clock(COPY_FEEDBACK_TTL);
    cx.run_until_parked();
    assert!(!pane.read_with(cx, |pane, cx| pane.mobile.copy_feedback.read(cx).is_copied()));
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert_eq!(cx.debug_bounds("mobile-qr").unwrap(), qr, "copy feedback must not move the QR");

    // 网络切换尚未完成时，不能再把旧邀请交给手机。
    cx.update(|_, cx| {
        pane.update(cx, |pane, cx| {
            pane.mobile.operation = true;
            cx.notify();
        });
        cx.write_to_clipboard(gpui::ClipboardItem::new_string("unchanged while switching".into()));
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let blocked_copy = cx.debug_bounds("mobile-copy-invitation").unwrap();
    cx.simulate_click(blocked_copy.center(), gpui::Modifiers::default());
    assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), "unchanged while switching");
    cx.update(|_, cx| {
        pane.update(cx, |pane, cx| {
            pane.mobile.operation = false;
            cx.notify();
        });
    });

    cx.simulate_resize(gpui::size(px(820.0), px(1200.0)));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let narrow_network = cx
        .debug_bounds("mobile-interface-row")
        .expect("network selection remains reachable in a narrow window");
    assert!(narrow_network.origin.x >= px(0.0) && narrow_network.right() <= px(820.0));
    let narrow_qr = cx.debug_bounds("mobile-qr").unwrap();
    assert!(narrow_qr.origin.x >= px(0.0) && narrow_qr.right() <= px(820.0));
    assert_eq!(narrow_qr.size, qr.size, "narrow layouts stack instead of shrinking the QR");

    cx.update(|_, cx| {
        pane.update(cx, |pane, cx| {
            pane.mobile.display_snapshot(fixture(true, true));
            cx.notify();
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    assert_eq!(pane.read_with(cx, |pane, _| pane.mobile.phase()), Phase::Paired);
    assert!(cx.debug_bounds("mobile-pairing").is_none());
    assert!(cx.debug_bounds("mobile-add-phone").is_some());

    cx.simulate_resize(gpui::size(px(1280.0), px(1600.0)));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let notifications = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = notifications.clone();
    let _subscription =
        cx.update(|_, cx| cx.observe(&pane, move |_, _| observed.set(observed.get() + 1)));
    for selector in [
        "mobile-device-row",
        "mobile-lan-row",
        "mobile-interface-row",
        "mobile-port-row",
        "mobile-relay-row",
        "mobile-default-permission-row",
        "mobile-notifications-row",
        "mobile-pause-row",
    ] {
        let bounds = cx.debug_bounds(selector).expect("settings row has actual layout");
        let outside = gpui::point(px(1279.0), px(1599.0));
        cx.simulate_mouse_move(outside, None, gpui::Modifiers::default());
        cx.run_until_parked();
        notifications.set(0);

        // 指针落在行的留白，避免按钮的重绘掩盖行自身缺少 hover 通知；不推进轮询时钟。
        let inside = gpui::point(bounds.origin.x + px(4.0), bounds.center().y);
        cx.simulate_mouse_move(inside, None, gpui::Modifiers::default());
        cx.run_until_parked();
        assert!(notifications.get() > 0, "{selector} must repaint on hover without networking");

        notifications.set(0);
        cx.simulate_mouse_move(
            inside + gpui::point(px(1.0), px(1.0)),
            None,
            gpui::Modifiers::default(),
        );
        cx.run_until_parked();
        assert_eq!(notifications.get(), 0, "{selector} must not redraw for every pointer move");

        notifications.set(0);
        cx.simulate_mouse_move(outside, None, gpui::Modifiers::default());
        cx.run_until_parked();
        assert!(notifications.get() > 0, "{selector} must repaint when the pointer leaves");
    }
}

#[test]
#[ignore = "requires a native Windows desktop and PEBREL_MOBILE_QA_DIR"]
fn native_mobile_three_state_preview() {
    use gpui::{Bounds, WindowBounds, WindowOptions, point};
    use std::{
        path::PathBuf,
        sync::{Arc, Mutex},
        time::Duration,
    };

    let output = PathBuf::from(std::env::var_os("PEBREL_MOBILE_QA_DIR").expect("QA directory"));
    std::fs::create_dir_all(&output).unwrap();
    assert!(!output.join("off-ready.json").exists(), "use a fresh QA directory");
    let result = Arc::new(Mutex::new(0));
    let completed = result.clone();
    gpui_platform::application().with_assets(crate::gpui_shell::assets::NebulaAssets).run(
        move |cx| {
            gpui_component::init(cx);
            let mut settings = crate::gpui_shell::config::Settings::load(ThemeName::Nord);
            settings.ui_language = crate::display::UiLanguage::ZhCn;
            cx.set_global(settings);
            crate::gpui_shell::theme::apply_chrome_theme(cx);
            let mut owner = None;
            let handle = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                            point(px(60.0), px(60.0)),
                            gpui::size(px(1080.0), px(860.0)),
                        ))),
                        focus: false,
                        show: true,
                        ..Default::default()
                    },
                    |window, cx| {
                        let pane = cx.new(|cx| SettingsPane::new(window, cx));
                        pane.update(cx, |pane, cx| {
                            pane.active_section = 11;
                            // Snapshot-only preview: no listeners, credentials or pairing records.
                            pane.mobile.initialized = true;
                            pane.mobile.display_snapshot(fixture(false, false));
                            pane.mobile_set_addresses(fixture_addresses(), window, cx);
                        });
                        owner = Some(pane.clone());
                        cx.new(|cx| gpui_component::Root::new(pane, window, cx))
                    },
                )
                .unwrap();
            let pane = owner.unwrap();
            cx.spawn(async move |cx| {
                for (name, enabled, paired) in
                    [("off", false, false), ("pairing", true, false), ("paired", true, true)]
                {
                    cx.update_window(handle.into(), |_, window, cx| {
                        pane.update(cx, |pane, cx| {
                            pane.mobile.display_snapshot(fixture(enabled, paired));
                            cx.notify();
                        });
                        window.refresh();
                    })
                    .unwrap();
                    cx.background_executor().timer(Duration::from_millis(700)).await;
                    std::fs::write(
                        output.join(format!("{name}-ready.json")),
                        serde_json::to_vec(
                            &serde_json::json!({"pid": std::process::id(), "state": name}),
                        )
                        .unwrap(),
                    )
                    .unwrap();
                    for _ in 0..100 {
                        if output.join(format!("{name}-captured")).exists() {
                            *result.lock().unwrap() += 1;
                            break;
                        }
                        cx.background_executor().timer(Duration::from_millis(200)).await;
                    }
                }
                cx.update_window(handle.into(), |_, window, _| window.remove_window()).unwrap();
                drop(pane);
                cx.update(|cx| cx.quit());
            })
            .detach();
        },
    );
    assert_eq!(*completed.lock().unwrap(), 3, "all three native states were captured");
}
