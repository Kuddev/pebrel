use super::*;
use crate::gpui_shell::settings_fixture::{SettingsBytesGuard, lock_theme_studio};

struct SegmentsHost(Entity<SettingsPane>, &'static str);

impl Render for SegmentsHost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus = self.0.read(cx).focus_handle.clone();
        div().size_full().track_focus(&focus).child(self.0.update(cx, |pane, cx| {
            pane.select_row(self.1, "Choice", "Description", cx).into_any_element()
        }))
    }
}

#[gpui::test]
fn capsule_uses_inset_thumb_full_hit_targets_and_keyboard_selection(cx: &mut gpui::TestAppContext) {
    let _lock = lock_theme_studio();
    let _settings = SettingsBytesGuard::capture();
    nebula_settings::persist_keys(&[("tab_reveal", "slide".into()), ("language", "en-US".into())])
        .unwrap();
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_reduce_motion(true);
        cx.set_global(crate::gpui_shell::config::Settings::load(ThemeName::Nord));
    });
    let mut pane = None;
    let (_, window) = cx.add_window_view(|window, cx| {
        let settings = cx.new(|cx| SettingsPane::new(window, cx));
        pane = Some(settings.clone());
        let host = cx.new(|cx| {
            cx.observe(&settings, |_, _, cx| cx.notify()).detach();
            SegmentsHost(settings, "tab_reveal")
        });
        gpui_component::Root::new(host, window, cx)
    });
    let pane = pane.unwrap();
    window.simulate_resize(gpui::size(px(500.0), px(200.0)));
    // Short and long labels keep the same capsule interaction.
    for (value, selector) in [
        ("slide", "settings-choice-tab_reveal-slide"),
        ("instant", "settings-choice-tab_reveal-instant"),
        ("slide", "settings-choice-tab_reveal-slide"),
    ] {
        window.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let slot = window.debug_bounds(selector).unwrap();
        // Click the blank edge of the real button, not just its centered label.
        window.simulate_click(
            gpui::point(slot.right() - px(5.0), slot.center().y),
            gpui::Modifiers::default(),
        );
        window.run_until_parked();
        window.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let track = window.debug_bounds("settings-choices-tab_reveal").unwrap();
        let thumb = window.debug_bounds("settings-indicator-tab_reveal").unwrap();
        // 点击后行内撤销控件可能出现，浮块与按钮必须用同一帧的布局比较。
        let slot = window.debug_bounds(selector).unwrap();
        assert!((f32::from(thumb.left() - slot.left())).abs() < 0.1);
        assert!((f32::from(thumb.size.width - slot.size.width)).abs() < 0.1);
        assert_eq!(thumb.top(), slot.top());
        assert_eq!(thumb.size.height, slot.size.height);
        assert_eq!(thumb.top() - track.top(), px(TRACK_INSET));
        assert_eq!(track.bottom() - thumb.bottom(), px(TRACK_INSET));
        let (_, select, values) = pane.read_with(window, |pane, _| {
            pane.selects.iter().find(|(key, _, _)| *key == "tab_reveal").unwrap().clone()
        });
        let row = window.update(|_, cx| select.read(cx).selected_index(cx).unwrap().row);
        assert_eq!(values[row], value);
        assert_eq!(RuntimeSettings::load().tab_reveal.settings_value(), value);
    }
    window.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.sync_select("tab_reveal", "slide", window, cx);
            pane.focus_handle.focus(window, cx);
            cx.notify();
        });
        let _ = window.draw(cx);
    });
    window.simulate_keystrokes("tab tab enter");
    // GPUI activates buttons on release; simulate_keystrokes sends only key-down.
    window.simulate_event(gpui::KeyUpEvent { keystroke: gpui::Keystroke::parse("enter").unwrap() });
    window.run_until_parked();
    assert_eq!(RuntimeSettings::load().tab_reveal.settings_value(), "instant");
}

#[gpui::test]
fn completion_capsules_keep_all_chinese_choices_at_large_font_size(cx: &mut gpui::TestAppContext) {
    let _lock = lock_theme_studio();
    let _settings = SettingsBytesGuard::capture();
    nebula_settings::persist_keys(&[
        ("completion_style", "hybrid".into()),
        ("language", "zh-CN".into()),
    ])
    .unwrap();
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_reduce_motion(true);
        gpui_component::Theme::global_mut(cx).font_size = px(28.0);
        cx.set_global(crate::gpui_shell::config::Settings::load(ThemeName::Nord));
    });
    let (_, window) = cx.add_window_view(|window, cx| {
        let pane = cx.new(|cx| SettingsPane::new(window, cx));
        let host = cx.new(|cx| {
            cx.observe(&pane, |_, _, cx| cx.notify()).detach();
            SegmentsHost(pane, "completion_style")
        });
        gpui_component::Root::new(host, window, cx)
    });
    window.simulate_resize(gpui::size(px(500.0), px(400.0)));
    for (value, selector) in [
        ("inline", "settings-choice-completion_style-inline"),
        ("popup", "settings-choice-completion_style-popup"),
        ("hybrid", "settings-choice-completion_style-hybrid"),
    ] {
        window.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert!(window.debug_bounds("settings-segments-dropdown-completion_style").is_none());
        let track = window.debug_bounds("settings-choices-completion_style").unwrap();
        assert!(track.right() <= px(500.0));
        assert!(track.size.width > px(SETTINGS_SELECT_WIDTH));
        let slot = window.debug_bounds(selector).unwrap();
        assert!(slot.left() >= track.left() && slot.right() <= track.right());
        window.simulate_click(slot.center(), gpui::Modifiers::default());
        window.run_until_parked();
        assert_eq!(RuntimeSettings::load().completion_style.settings_value(), value);
    }
}

#[gpui::test]
fn rapid_retarget_keeps_the_visible_thumb_position(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| gpui_component::init(cx));
    struct MotionHost {
        selected: usize,
        motion: Option<Entity<IndicatorMotion>>,
    }
    impl Render for MotionHost {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let control = SettingsSegments {
                key: "probe",
                selected: self.selected,
                height: px(28.0),
                labels: vec!["A".into(), "B".into(), "C".into()],
                buttons: (0usize..3).map(|ix| Button::new(ix).flex_1().h(px(28.0))).collect(),
            }
            .render(window, cx)
            .into_any_element();
            self.motion = Some(window.use_keyed_state(
                "settings-indicator-motion-probe",
                cx,
                |_, _| unreachable!(),
            ));
            control
        }
    }
    let mut host_out = None;
    let (_, window) = cx.add_window_view(|window, cx| {
        let host = cx.new(|_| MotionHost { selected: 0, motion: None });
        host_out = Some(host.clone());
        gpui_component::Root::new(host, window, cx)
    });
    let host = host_out.unwrap();
    window.update(|window, cx| {
        let _ = window.draw(cx);
    });
    window.update(|_, cx| {
        host.update(cx, |host, cx| {
            host.selected = 2;
            cx.notify();
        });
    });
    window.update(|window, cx| {
        let _ = window.draw(cx);
    });
    window.update(|_, cx| {
        let state = host.read(cx).motion.as_ref().unwrap().clone();
        state.read(cx).position.set(0.27);
        host.update(cx, |host, cx| {
            host.selected = 1;
            cx.notify();
        });
    });
    window.update(|window, cx| {
        let _ = window.draw(cx);
        let state = host.read(cx).motion.as_ref().unwrap();
        let motion: &IndicatorMotion = state.read(cx);
        assert_eq!(motion.from, 0.27);
        assert_eq!(motion.target, 1.0 / 3.0);
    });
    let thumb = window.debug_bounds("settings-indicator-probe").unwrap();
    let track = window.debug_bounds("settings-choices-probe").unwrap();
    assert!(thumb.left() > track.left() + px(TRACK_INSET));
    assert!(thumb.right() < track.right() - px(TRACK_INSET));
}
