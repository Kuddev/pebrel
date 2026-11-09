//! SFTP 面板的拖入遮罩：拖着本地文件经过面板时，压暗当前列表、铺一层中心
//! 渐隐的点阵，并用虚线框写明“松开会传到哪里”。
//!
//! 遮罩平时 `invisible`，只靠 `group_drag_over` 在拖动期间显形——没有任何
//! 额外状态，拖走或松手时自然消失。它不注册命中区，面板与各目录行原有的
//! `on_drop` 照常接收；落在某个目录行上时，那一行的高亮透过半透明底可见，
//! 上传目标以行为准。GPUI 没有背景模糊，用半透明窗框色压暗代替。

use super::*;
use gpui::{Bounds, canvas, fill, point, size};

pub(super) const DROP_GROUP: &str = "remote-files-drop";
const DOT_PITCH: f32 = 14.0;
const FRAME_INSET: f32 = 10.0;

impl NebulaWorkspace {
    pub(super) fn render_remote_drop_overlay(&self, cx: &Context<Self>) -> gpui::AnyElement {
        let theme = cx.theme();
        let accent = theme.primary;
        let language = crate::gpui_shell::config::ui_language(cx);
        let destination = if self.remote_browser.path.is_empty() {
            self.remote_browser.destination.clone()
        } else {
            format!("{}:{}", self.remote_browser.destination, self.remote_browser.path)
        };
        let mut veil = theme.background;
        veil.a = 0.6;
        let dot = accent.opacity(0.34);
        let lift = crate::gpui_shell::theme::raised_fill(cx);
        let lift_shadow = crate::gpui_shell::theme::settings_card_shadow(cx);

        let overlay = div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .invisible()
            .bg(veil)
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        // 点阵从中心向外渐隐：只在框内铺，边缘让给面板原有内容。
                        let center = bounds.center();
                        let reach = f32::from(bounds.size.width.max(bounds.size.height)) * 0.5;
                        let cols = (f32::from(bounds.size.width) / DOT_PITCH) as i32;
                        let rows = (f32::from(bounds.size.height) / DOT_PITCH) as i32;
                        for row in 0..=rows {
                            for col in 0..=cols {
                                let x = bounds.origin.x + px(col as f32 * DOT_PITCH + DOT_PITCH / 2.0);
                                let y = bounds.origin.y + px(row as f32 * DOT_PITCH + DOT_PITCH / 2.0);
                                let dx = f32::from(x - center.x);
                                let dy = f32::from(y - center.y);
                                let fade = 1.0 - ((dx * dx + dy * dy).sqrt() / reach).clamp(0.0, 1.0);
                                if fade <= 0.05 {
                                    continue;
                                }
                                window.paint_quad(
                                    fill(
                                        Bounds::new(point(x - px(1.0), y - px(1.0)), size(px(2.0), px(2.0))),
                                        dot.opacity(fade),
                                    )
                                    .corner_radii(gpui::Corners::all(px(1.0))),
                                );
                            }
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
            .child(
                v_flex()
                    .absolute()
                    .top(px(FRAME_INSET))
                    .left(px(FRAME_INSET))
                    .right(px(FRAME_INSET))
                    .bottom(px(FRAME_INSET))
                    .items_center()
                    .justify_center()
                    .gap(px(10.0))
                    .rounded(px(12.0))
                    .border_1()
                    .border_dashed()
                    .border_color(accent.opacity(0.7))
                    .bg(accent.opacity(0.05))
                    .child(
                        div()
                            .size(px(44.0))
                            .rounded(px(12.0))
                            .bg(lift)
                            .shadow(lift_shadow)
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(accent)
                            .child(Icon::new(IconName::ArrowUp).size(px(22.0))),
                    )
                    .child(
                        div()
                            .text_color(theme.foreground)
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(language.text(crate::i18n::Message::TransferUploadHint)),
                    )
                    .child(
                        div()
                            .max_w_full()
                            .px(px(16.0))
                            .truncate()
                            .font_family(theme.mono_font_family.clone())
                            .text_xs()
                            .text_color(accent)
                            .child(destination),
                    ),
            );
        overlay
            .group_drag_over::<ExternalPaths>(DROP_GROUP, |style| style.visible())
            .group_drag_over::<crate::gpui_shell::file_drop::FileTreeDrag>(DROP_GROUP, |style| {
                style.visible()
            })
            .into_any_element()
    }
}
