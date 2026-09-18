use super::*;
use crate::gpui_shell::config::Settings;
use crate::i18n::Message;
use nebula_settings::{
    MAX_SCROLL_SPEED, MIN_SCROLL_SPEED, SCROLL_SPEED_STEP, normalize_scroll_speed,
};

impl SettingsPane {
    pub(super) fn commit_scrollback_lines(
        &mut self,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = self.try_persist(&[("scrollback_lines", value.to_owned())], cx) {
            let language = crate::gpui_shell::config::ui_language(cx);
            crate::gpui_shell::toast::toast(
                window,
                cx,
                crate::gpui_shell::toast::ToastKind::Warning,
                language.format(Message::SettingsSaveFailed, &[("error", &error.to_string())]),
            );
        }
        self.sync_select(
            "scrollback_lines",
            &self.runtime.scrollback_lines.to_string(),
            window,
            cx,
        );
    }

    pub(super) fn create_scroll_speed_slider(
        runtime: &RuntimeSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
        subscriptions: &mut Vec<Subscription>,
    ) -> Entity<SliderState> {
        let slider = cx.new(|_| {
            SliderState::new()
                .min(MIN_SCROLL_SPEED)
                .max(MAX_SCROLL_SPEED)
                .step(SCROLL_SPEED_STEP)
                .default_value(runtime.scroll_speed)
        });
        subscriptions.push(cx.subscribe_in(
            &slider,
            window,
            |this, _, event: &SliderEvent, window, cx| match event {
                SliderEvent::Change(value) => this.preview_scroll_speed(value.start(), cx),
                SliderEvent::Release(value) => this.commit_scroll_speed(value.start(), window, cx),
            },
        ));
        // Accessibility increment/decrement uses SliderState::set_value instead
        // of emitting Change/Release. Mouse and keyboard previews already match
        // runtime, so only these otherwise-unhandled value changes are saved here.
        subscriptions.push(cx.observe_in(&slider, window, |this, slider, window, cx| {
            let value = slider.read(cx).value().start();
            if (value - this.runtime.scroll_speed).abs() > f32::EPSILON {
                this.preview_scroll_speed(value, cx);
                this.commit_scroll_speed(value, window, cx);
            }
        }));
        slider
    }

    fn preview_scroll_speed(&mut self, value: f32, cx: &mut Context<Self>) {
        let speed = normalize_scroll_speed(value);
        self.runtime.scroll_speed = speed;
        cx.global_mut::<Settings>().scroll_speed = speed;
        cx.notify();
    }

    fn commit_scroll_speed(&mut self, value: f32, window: &mut Window, cx: &mut Context<Self>) {
        let speed = normalize_scroll_speed(value);
        if let Err(error) = self.try_persist(&[("scroll_speed", format!("{speed:.2}"))], cx) {
            let saved = RuntimeSettings::load().scroll_speed;
            self.preview_scroll_speed(saved, cx);
            self.scroll_speed_slider.update(cx, |state, cx| state.set_value(saved, window, cx));
            let language = crate::gpui_shell::config::ui_language(cx);
            crate::gpui_shell::toast::toast(
                window,
                cx,
                crate::gpui_shell::toast::ToastKind::Warning,
                language.format(Message::SettingsSaveFailed, &[("error", &error.to_string())]),
            );
        }
    }

    pub(super) fn scroll_speed_row(&self, cx: &Context<Self>) -> impl IntoElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let control = h_flex()
            .id("scroll-speed-control")
            .debug_selector(|| "scroll-speed-control".to_owned())
            .aria_label(language.text(Message::SettingsScrollingSpeed))
            .track_focus(&self.scroll_speed_focus)
            .w(px(220.0))
            .items_center()
            .gap_3()
            .border_1()
            .border_color(gpui::transparent_black())
            .rounded_md()
            .focus(|style| style.border_color(cx.theme().ring))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    window.focus(&this.scroll_speed_focus, cx);
                }),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let value = match event.keystroke.key.as_str() {
                    "left" | "down" => this.runtime.scroll_speed - SCROLL_SPEED_STEP,
                    "right" | "up" => this.runtime.scroll_speed + SCROLL_SPEED_STEP,
                    "home" => MIN_SCROLL_SPEED,
                    "end" => MAX_SCROLL_SPEED,
                    _ => return,
                };
                let value = normalize_scroll_speed(value);
                this.preview_scroll_speed(value, cx);
                this.scroll_speed_slider.update(cx, |state, cx| state.set_value(value, window, cx));
                this.commit_scroll_speed(value, window, cx);
                cx.stop_propagation();
            }))
            .child(div().flex_1().min_w_0().child(Slider::new(&self.scroll_speed_slider)))
            .child(
                div()
                    .w(px(48.0))
                    .flex_shrink_0()
                    .child(format!("{:.2}×", self.runtime.scroll_speed)),
            );
        self.row(
            language.text(Message::SettingsScrollingSpeed),
            language.text(Message::SettingsScrollingSpeedDescription),
            control,
            cx,
        )
    }
}
