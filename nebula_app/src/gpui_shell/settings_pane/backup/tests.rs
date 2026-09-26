use super::*;

#[gpui::test]
fn pairing_design_backup_groups_scope_and_save_feedback_with_storage(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_global(crate::gpui_shell::config::Settings::load(ThemeName::Nord));
    });
    let mut pane = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| SettingsPane::new(window, cx));
        view.update(cx, |pane, _| {
            // Exercise real layout and hitboxes without touching credentials or storage.
            pane.backup_ui.initialized = true;
            pane.backup_ui.configuration = true;
            pane.backup_remote.protocol = BackupProtocol::Off;
            pane.active_section = 9;
        });
        pane = Some(view.clone());
        gpui_component::Root::new(view, window, cx)
    });
    let pane = pane.unwrap();
    for width in [1280.0, 800.0] {
        cx.simulate_resize(gpui::size(px(width), px(1400.0)));
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
        let card = cx.debug_bounds("cloud-storage-card").expect("storage configuration");
        assert!(card.right() <= px(width));
        assert!(card.size.width <= px(720.0));
        let center = px((width + SETTINGS_NAV_WIDTH) / 2.0);
        assert!((f32::from(card.center().x - center)).abs() <= 2.0);
        let provider = cx.debug_bounds("cloud-provider").expect("bounded provider dropdown");
        assert!(provider.size.width <= px(220.0));
        assert!(provider.left() >= card.left() && provider.right() <= card.right());
        for selector in ["cloud-scope-toggle", "cloud-save-status"] {
            let control = cx.debug_bounds(selector).expect("in-card control");
            assert!(control.origin.x >= card.origin.x && control.right() <= card.right());
            assert!(control.origin.y >= card.origin.y && control.bottom() <= card.bottom());
        }
        assert!(cx.debug_bounds("cloud-scope-options").is_none());
    }
    let toggle = cx.debug_bounds("cloud-scope-toggle").unwrap();
    let point = gpui::point(toggle.origin.x + px(4.0), toggle.center().y);
    cx.simulate_mouse_down(point, MouseButton::Left, gpui::Modifiers::default());
    cx.simulate_mouse_up(point, MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    assert!(pane.read_with(cx, |pane, _| pane.backup_ui.scope_open));
    let card = cx.debug_bounds("cloud-storage-card").unwrap();
    let scope = cx.debug_bounds("cloud-scope-options").unwrap();
    assert!(scope.origin.y >= card.origin.y && scope.bottom() <= card.bottom());
    let history = cx.debug_bounds("cloud-tab-snapshots").unwrap();
    cx.simulate_mouse_down(history.center(), MouseButton::Left, gpui::Modifiers::default());
    cx.simulate_mouse_up(history.center(), MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    assert!(!pane.read_with(cx, |pane, _| pane.backup_ui.configuration));
    assert!(pane.read_with(cx, |pane, _| pane.backup_ui.snapshots.is_empty()));
}
