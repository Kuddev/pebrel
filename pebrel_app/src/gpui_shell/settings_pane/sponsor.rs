//! 设置首页「项目与支持」下的赞助商页：左侧品牌贴图、右侧介绍与注册链接。
//!
//! 不做成外链行：外链行一点就跳浏览器，赞助商介绍在应用里就该读得到；
//! 页面只提供一条明确的注册链接，点它才离开应用。

use std::sync::{Arc, OnceLock};

use super::*;
use crate::i18n::Message;

/// 与中英文 README 保持一致的 AIAPI 邀请链接。
const SPONSOR_URL: &str = "https://cn.aiapi.bot/invite?code=MCYLZ5H6FR";
const SPONSOR_NAME: &str = "AIAPI";
const LOGO_PNG: &[u8] = include_bytes!("../../../../extra/logo/sponsor_aiapi.png");
const LOGO_WIDTH: f32 = 188.0;

/// 贴图只解码一次：RGBA8 → BGRA（gpui 帧通道序），与壁纸 / 文档图同款。
fn sponsor_logo() -> Option<Arc<RenderImage>> {
    static LOGO: OnceLock<Option<Arc<RenderImage>>> = OnceLock::new();
    LOGO.get_or_init(|| {
        let mut rgba = image::load_from_memory(LOGO_PNG).ok()?.into_rgba8();
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        Some(Arc::new(RenderImage::new([image::Frame::new(rgba)])))
    })
    .clone()
}

impl SettingsPane {
    pub(super) fn open_sponsor_page(&mut self, cx: &mut Context<Self>) {
        self.about_sponsor_open = true;
        cx.notify();
    }

    pub(super) fn close_sponsor_page(&mut self, cx: &mut Context<Self>) {
        self.about_sponsor_open = false;
        cx.notify();
    }

    pub(super) fn section_sponsor(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let language = crate::gpui_shell::config::ui_language(cx);
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let ink = theme.foreground;
        let base_px = self.font_size_px(cx);

        let back = h_flex()
            .id("sponsor-back")
            .w_auto()
            .h(px(32.0))
            .px_2()
            .gap_2()
            .items_center()
            .rounded_md()
            .cursor_pointer()
            .text_color(muted)
            .hover(move |row| row.text_color(ink))
            .on_click(cx.listener(|this, _, _, cx| this.close_sponsor_page(cx)))
            .child(Icon::new(IconName::ArrowLeft).small())
            .child(language.text(Message::ThemeEditorBack));

        let logo =
            div().w(px(LOGO_WIDTH)).flex_shrink_0().when_some(sponsor_logo(), |slot, image| {
                slot.child(gpui::StyledImage::object_fit(
                    img(image).w(px(LOGO_WIDTH)).h(px(LOGO_WIDTH * 893.0 / 2400.0)),
                    gpui::ObjectFit::Contain,
                ))
            });

        let register = Button::new("sponsor-aiapi-register")
            .text()
            .small()
            .w_auto()
            .h(px(32.0))
            .px_0()
            .label(language.text(Message::SettingsSponsorVisit))
            .icon(IconName::ExternalLink)
            .on_click(|_, _, cx| cx.open_url(SPONSOR_URL));

        let copy = v_flex()
            .flex_1()
            .min_w_0()
            .gap(px(10.0))
            .child(
                div()
                    .text_size(px(base_px * 1.3))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(ink)
                    .child(SPONSOR_NAME),
            )
            .child(
                div().text_color(ink).child(language.text(Message::SettingsSponsorAiapiHeadline)),
            )
            .child(
                div()
                    .text_color(muted)
                    .line_height(px(base_px * 1.6))
                    .child(language.text(Message::SettingsSponsorAiapiDescription)),
            )
            .child(register);

        v_flex()
            .w_full()
            .gap(px(20.0))
            .child(back)
            .child(
                div()
                    .h(px(30.0))
                    .text_size(px(base_px * 0.85))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(muted)
                    .child(language.text(Message::SettingsAboutSponsors)),
            )
            .child(h_flex().w_full().items_start().gap(px(28.0)).child(logo).child(copy))
    }
}
