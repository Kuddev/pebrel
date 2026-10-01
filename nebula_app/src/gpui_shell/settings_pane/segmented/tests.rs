use super::*;
use crate::gpui_shell::settings_fixture::{SettingsBytesGuard, lock_theme_studio};

struct SegmentsHost(Entity<SettingsPane>);

impl Render for SegmentsHost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus = self.0.read(cx).focus_handle.clone();
        div()
            .size_full()
            .track_focus(&focus)
            .child(self.0.update(cx, |pane, cx| pane.segmented_setting("vcs_display", cx).unwrap()))
    }
}

#[gpui::test]
fn capsule_uses_inset_thumb_full_hit_targets_and_keyboard_selection(cx: &mut gpui::TestAppContext) {
    let _lock = lock_theme_studio();
    let _settings = SettingsBytesGuard::capture();
    nebula_settings::persist_keys(&[("vcs_display", "auto".into())]).unwrap();
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
            SegmentsHost(settings)
        });
        gpui_component::Root::new(host, window, cx)
    });
    let pane = pane.unwrap();
    window.simulate_resize(gpui::size(px(500.0), px(200.0)));
    for (value, selector) in [
        ("auto", "settings-choice-vcs_display-auto"),
        ("svn", "settings-choice-vcs_display-svn"),
        ("git", "settings-choice-vcs_display-git"),
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
        let track = window.debug_bounds("settings-choices-vcs_display").unwrap();
        let thumb = window.debug_bounds("settings-indicator-vcs_display").unwrap();
        assert!((f32::from(thumb.left() - slot.left())).abs() < 0.1);
        assert!((f32::from(thumb.size.width - slot.size.width)).abs() < 0.1);
        assert_eq!(thumb.top(), slot.top());
        assert_eq!(thumb.size.height, slot.size.height);
        assert_eq!(thumb.top() - track.top(), px(TRACK_INSET));
        assert_eq!(track.bottom() - thumb.bottom(), px(TRACK_INSET));
        let (_, select, values) = pane.read_with(window, |pane, _| {
            pane.selects.iter().find(|(key, _, _)| *key == "vcs_display").unwrap().clone()
        });
        let row = window.update(|_, cx| select.read(cx).selected_index(cx).unwrap().row);
        assert_eq!(values[row], value);
        assert_eq!(RuntimeSettings::load().vcs_display.settings_value(), value);
    }
    window.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.sync_select("vcs_display", "auto", window, cx);
            pane.focus_handle.focus(window, cx);
            cx.notify();
        });
        let _ = window.draw(cx);
    });
    window.simulate_keystrokes("tab tab tab enter");
    // GPUI activates buttons on release; simulate_keystrokes sends only key-down.
    window.simulate_event(gpui::KeyUpEvent { keystroke: gpui::Keystroke::parse("enter").unwrap() });
    window.run_until_parked();
    assert_eq!(RuntimeSettings::load().vcs_display.settings_value(), "svn");
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
