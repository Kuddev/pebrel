use super::*;

fn waiting() -> connection::Snapshot {
    connection::Snapshot {
        mode: Mode::Relay,
        status: Status::Waiting,
        invitation: "test-qr".to_owned(),
        address: None,
        allow_input: false,
    }
}

#[test]
fn mobile_pairing_qr_requires_a_live_invitation_and_ready_connection() {
    let mut snapshot = waiting();
    assert!(pairing_board::qr_available(Some(&snapshot), Some(400), 399));
    assert!(!pairing_board::qr_available(Some(&snapshot), Some(400), 400));
    for status in [Status::Starting, Status::Reconnecting, Status::Failed, Status::Stopped] {
        snapshot.status = status;
        assert!(!pairing_board::qr_available(Some(&snapshot), Some(400), 200));
    }
    snapshot.status = Status::Connected;
    snapshot.invitation.clear();
    assert!(!pairing_board::qr_available(Some(&snapshot), Some(400), 200));
    assert!(!pairing_board::qr_available(None, None, 200));
}

#[test]
fn mobile_pairing_lan_does_not_invent_a_five_minute_expiry() {
    let mut snapshot = waiting();
    snapshot.mode = Mode::Lan;
    assert!(pairing_board::qr_available(Some(&snapshot), None, 10_000));
}

#[cfg(feature = "gpui-test-support")]
#[gpui::test]
fn pairing_design_mobile_keeps_qr_visible_and_credentials_collapsed(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_global(crate::gpui_shell::config::Settings::load(ThemeName::Nord));
    });
    let mut pane = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| SettingsPane::new(window, cx));
        view.update(cx, |pane, _| {
            // No real listeners, credentials, network or monitor in a layout test.
            pane.mobile.initialized = true;
            pane.mobile.mode = Mode::Relay;
            pane.active_section = 10;
        });
        pane = Some(view.clone());
        gpui_component::Root::new(view, window, cx)
    });
    let pane = pane.unwrap();
    for width in [1280.0, 800.0] {
        cx.simulate_resize(gpui::size(px(width), px(1600.0)));
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        let qr = cx.debug_bounds("mobile-qr-surface").expect("prominent QR surface");
        assert!(qr.size.width >= px(240.0) && qr.size.height >= px(240.0));
        assert!(qr.right() <= px(width) && qr.bottom() <= px(1600.0));
        if width >= 1040.0 {
            let config = cx.debug_bounds("mobile-config-scroll").unwrap();
            assert!(qr.origin.x >= config.right());
        } else {
            assert!(cx.debug_bounds("mobile-config-scroll").is_none());
        }
        assert!(cx.debug_bounds("mobile-advanced-field-2").is_none());
    }
    cx.simulate_resize(gpui::size(px(1280.0), px(1600.0)));
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    let advanced = cx.debug_bounds("mobile-advanced").expect("advanced toggle hitbox");
    // Include the surrounding whitespace, not just the text label.
    let point = gpui::point(advanced.origin.x + px(6.0), advanced.center().y);
    cx.simulate_mouse_down(point, MouseButton::Left, gpui::Modifiers::default());
    cx.simulate_mouse_up(point, MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    assert!(pane.read_with(cx, |pane, _| pane.mobile.advanced));
    assert!(cx.debug_bounds("mobile-advanced-field-2").is_some());
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.settings_search_input.update(cx, |input, cx| input.set_value("font", window, cx));
            pane.open_mobile(window, cx);
        })
    });
    cx.run_until_parked();
    assert_eq!(pane.read_with(cx, |pane, _| pane.active_section), 10);
    assert!(pane.read_with(cx, |pane, cx| pane.settings_search_input.read(cx).value().is_empty()));
}
