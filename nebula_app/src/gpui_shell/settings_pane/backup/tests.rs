use super::*;

fn draw(cx: &mut gpui::VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

fn click(cx: &mut gpui::VisualTestContext, selector: &'static str) {
    let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("missing {selector}"));
    // 留白同样属于真实点击区域，不能只测文本中点。
    let point = gpui::point(bounds.left() + px(6.0), bounds.center().y);
    cx.simulate_mouse_down(point, MouseButton::Left, gpui::Modifiers::default());
    cx.simulate_mouse_up(point, MouseButton::Left, gpui::Modifiers::default());
    draw(cx);
}

#[gpui::test]
fn backup_wizard_requires_connection_and_matching_passwords(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_reduce_motion(true);
        cx.set_global(crate::gpui_shell::config::Settings::load(ThemeName::Nord));
    });
    let mut pane = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| SettingsPane::new(window, cx));
        view.update(cx, |pane, _| {
            pane.backup_remote = BackupRemoteConfig::default();
            pane.active_section = 9;
        });
        pane = Some(view.clone());
        gpui_component::Root::new(view, window, cx)
    });
    let pane = pane.unwrap();
    for width in [1280.0, 800.0] {
        cx.simulate_resize(gpui::size(px(width), px(1400.0)));
        draw(cx);
        let wizard = cx.debug_bounds("backup-wizard").unwrap();
        assert!(wizard.right() <= px(width));
        assert!(wizard.size.width <= px(700.0));
        let center = px((width + SETTINGS_NAV_WIDTH) / 2.0);
        assert!(f32::from(wizard.center().x - center).abs() <= 2.0);
        assert!(cx.debug_bounds("backup-content-0").is_none());
    }
    click(cx, "backup-provider-4");
    click(cx, "backup-next");
    assert_eq!(pane.read_with(cx, |p, _| p.backup_ui.step), 2);
    click(cx, "backup-next");
    assert_eq!(pane.read_with(cx, |p, _| p.backup_ui.step), 2);
    assert!(
        pane.read_with(cx, |p, _| p.backup_status.as_ref().is_some_and(BackupStatus::is_error))
    );
    let folder = tempfile::tempdir().unwrap();
    cx.update(|window, cx| {
        pane.update(cx, |p, cx| {
            p.backup_remote_inputs[0].update(cx, |input, cx| {
                input.set_value(folder.path().to_string_lossy().into_owned(), window, cx)
            });
        })
    });
    click(cx, "backup-next");
    assert_eq!(pane.read_with(cx, |p, _| p.backup_ui.step), 3);
    assert_eq!(pane.read_with(cx, |p, _| p.backup_remote.protocol), BackupProtocol::Off);
    click(cx, "backup-back");
    assert_eq!(
        pane.read_with(cx, |p, cx| p.backup_remote_inputs[0].read(cx).value().to_string()),
        folder.path().to_string_lossy()
    );
    click(cx, "backup-next");
    click(cx, "backup-next");
    assert_eq!(pane.read_with(cx, |p, _| p.backup_ui.step), 3);
    cx.update(|window, cx| {
        pane.update(cx, |p, cx| {
            p.backup_pass_input
                .update(cx, |input, cx| input.set_value("correct horse", window, cx));
            p.backup_ui
                .confirm
                .as_ref()
                .unwrap()
                .update(cx, |input, cx| input.set_value("different horse", window, cx));
        })
    });
    click(cx, "backup-next");
    assert_eq!(pane.read_with(cx, |p, _| p.backup_ui.step), 3);
    cx.update(|window, cx| {
        pane.update(cx, |p, cx| {
            p.backup_ui
                .confirm
                .as_ref()
                .unwrap()
                .update(cx, |input, cx| input.set_value("correct horse", window, cx));
        })
    });
    click(cx, "backup-next");
    assert_eq!(pane.read_with(cx, |p, _| p.backup_ui.step), 4);
    assert_eq!(pane.read_with(cx, |p, _| p.backup_selection), recommended());
    assert!(cx.debug_bounds("backup-later").is_some());
    click(cx, "backup-content-0");
    assert!(!pane.read_with(cx, |p, _| p.backup_selection.appearance));
    assert_eq!(pane.read_with(cx, |p, _| p.backup_remote.protocol), BackupProtocol::Off);
}

#[gpui::test]
fn backup_drawer_keeps_scope_off_dashboard_and_cancel_discards_draft(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_reduce_motion(true);
        cx.set_global(crate::gpui_shell::config::Settings::load(ThemeName::Nord));
    });
    let mut pane = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| SettingsPane::new(window, cx));
        view.update(cx, |p, cx| {
            p.backup_remote = BackupRemoteConfig::default();
            p.initialize_backup(window, cx);
            p.backup_remote.protocol = BackupProtocol::Folder;
            p.backup_remote.selection = recommended();
            p.active_section = 9;
        });
        pane = Some(view.clone());
        gpui_component::Root::new(view, window, cx)
    });
    let pane = pane.unwrap();
    cx.simulate_resize(gpui::size(px(1100.0), px(1000.0)));
    draw(cx);
    assert!(cx.debug_bounds("backup-content-0").is_none());
    click(cx, "backup-now");
    let drawer = cx.debug_bounds("backup-drawer").unwrap();
    assert_eq!(drawer.size.width, px(440.0));
    assert!(drawer.right() <= px(1100.0));
    click(cx, "backup-content-0");
    assert!(!pane.read_with(cx, |p, _| p.backup_selection.appearance));
    click(cx, "backup-cancel");
    assert!(cx.debug_bounds("backup-drawer").is_none());
    assert_eq!(pane.read_with(cx, |p, _| p.backup_selection), recommended());
    assert_eq!(pane.read_with(cx, |p, _| p.backup_remote.selection), recommended());
}

#[gpui::test]
fn backup_form_columns_and_password_group_follow_prototype(cx: &mut gpui::TestAppContext) {
    check_backup_form_layout(cx, crate::display::UiLanguage::EnUs);
}

#[gpui::test]
fn backup_form_columns_and_password_group_in_chinese(cx: &mut gpui::TestAppContext) {
    check_backup_form_layout(cx, crate::display::UiLanguage::ZhCn);
}

fn check_backup_form_layout(cx: &mut gpui::TestAppContext, language: crate::display::UiLanguage) {
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_reduce_motion(true);
        cx.set_global(crate::gpui_shell::config::Settings::load(ThemeName::Nord));
    });
    cx.update(|cx| cx.global_mut::<crate::gpui_shell::config::Settings>().ui_language = language);
    let mut pane = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| SettingsPane::new(window, cx));
        view.update(cx, |p, _| {
            p.backup_remote = BackupRemoteConfig::default();
            p.active_section = 9;
        });
        pane = Some(view.clone());
        gpui_component::Root::new(view, window, cx)
    });
    let pane = pane.unwrap();
    for width in [800.0, 1280.0] {
        cx.update(|_, cx| {
            pane.update(cx, |p, cx| {
                p.backup_ui.step = 1;
                cx.notify();
            })
        });
        cx.simulate_resize(gpui::size(px(width), px(1400.0)));
        draw(cx);
        let folder = cx.debug_bounds("backup-provider-4").unwrap();
        let next = cx.debug_bounds("backup-next").unwrap();
        let wizard = cx.debug_bounds("backup-wizard").unwrap();
        assert!(
            next.bottom() <= wizard.bottom(),
            "footer is clipped at {width}: {next:?}, {wizard:?}"
        );
        assert!(
            folder.bottom() <= next.top(),
            "storage choices overlap the footer at {width}: {folder:?}, {next:?}"
        );
        click(cx, "backup-provider-4");
        assert_eq!(pane.read_with(cx, |p, _| p.backup_ui.draft.protocol), BackupProtocol::Folder);
        click(cx, "backup-provider-1");
        assert_eq!(
            pane.read_with(cx, |p, _| view::provider(&p.backup_ui.draft)),
            Message::CloudWebdav
        );
        click(cx, "backup-next");
        assert_eq!(pane.read_with(cx, |p, _| p.backup_ui.step), 2);
        assert_eq!(pane.read_with(cx, |p, _| p.backup_ui.draft.protocol), BackupProtocol::WebDav);
        for selector in
            ["backup-field-control-0", "backup-field-control-1", "backup-field-control-2"]
        {
            assert!(cx.debug_bounds(selector).is_some(), "missing {selector} at {width}");
        }
    }
    for width in [1280.0, 800.0] {
        cx.simulate_resize(gpui::size(px(width), px(1400.0)));
        draw(cx);
        let first = cx.debug_bounds("backup-field-control-0").unwrap();
        for (control, label) in [
            ("backup-field-control-0", "backup-field-label-0"),
            ("backup-field-control-1", "backup-field-label-1"),
            ("backup-field-control-2", "backup-field-label-2"),
        ] {
            let control = cx.debug_bounds(control).unwrap_or_else(|| {
                let state =
                    pane.read_with(cx, |p, _| (p.backup_ui.step, p.backup_ui.draft.protocol));
                panic!("missing {control} at width {width}, wizard state {state:?}")
            });
            let label = cx.debug_bounds(label).unwrap();
            assert!((control.left() - first.left()).abs() <= px(1.0));
            assert!((control.right() - first.right()).abs() <= px(1.0));
            if width >= 960.0 {
                assert!(label.right() < control.left());
                assert_eq!(control.size.width, px(300.0));
            } else {
                assert!(label.bottom() <= control.top());
            }
        }
        cx.update(|_, cx| {
            pane.update(cx, |p, cx| {
                p.backup_ui.step = 3;
                cx.notify();
            })
        });
        draw(cx);
        let wizard = cx.debug_bounds("backup-wizard").unwrap();
        for selector in ["backup-wizard-heading", "backup-password-form", "backup-password-note"] {
            let bounds = cx.debug_bounds(selector).unwrap();
            assert!((bounds.center().x - wizard.center().x).abs() <= px(1.0));
            assert!(bounds.left() >= wizard.left() && bounds.right() <= wizard.right());
        }
        click(cx, "backup-next");
        assert_eq!(pane.read_with(cx, |p, _| p.backup_ui.step), 3);
        cx.update(|_, cx| {
            pane.update(cx, |p, cx| {
                p.backup_ui.step = 2;
                cx.notify();
            })
        });
    }
}
