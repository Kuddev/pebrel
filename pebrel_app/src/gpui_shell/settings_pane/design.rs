//! 设置页的设计 token 与行原语。
//!
//! 每组设置使用一张卡片，标题在卡外，组内分隔线与行内容对齐。
//! 窄窗中的控件可以换行；内容密度只调整留白，不改变字号和命中区域。

use gpui::FontWeight;
use gpui_component::button::ButtonCustomVariant;

use super::*;

/// 说明文字相对正文的字号比。全页只有两档字号：正文走用户基准字号，说明小
/// 一档。
pub(super) const DESC_SCALE: f32 = 0.82;
/// Parent headings stay above item labels in the visual hierarchy.
const GROUP_TITLE_SCALE: f32 = 16.0 / 14.0;
/// 卡外分组标题比卡内条目略小、更淡：卡片本身已经说明了“这是一组”。
const CARD_TITLE_SCALE: f32 = 12.5 / 14.0;
const CARD_RADIUS: f32 = 10.0;
/// 卡内行的左右留白，同时是行间细线的内缩量。
pub(super) const CARD_PAD_X: f32 = 16.0;
/// label ↔ 说明。这 4px 在说"这两行是同一件事"。
const LABEL_DESC_GAP: f32 = 4.0;
/// 标准密度沿用原紧凑档的行留白；间距只由行内 padding 提供。
const ROW_PAD_Y: f32 = 12.0;
/// 紧凑档只再收紧少量留白，字号与控件命中区域保持不变。
const ROW_PAD_Y_COMPACT: f32 = 10.0;
/// 组与组。
pub(super) const GROUP_GAP: f32 = 28.0;
pub(super) const CONTENT_MAX_WIDTH: f32 = 960.0;
pub(super) const CONTENT_PAD_X: f32 = 32.0;
pub(super) const CONTENT_PAD_TOP: f32 = 36.0;
pub(super) const CONTENT_PAD_BOTTOM: f32 = 64.0;
/// 控件列宽。够放下最宽的下拉（220）加一点余量；开关这类窄控件在列内右对齐，
/// 右缘与下拉保持一致。文字列允许收缩换行，避免窄窗口被旧的 320px 下限撑宽。
const CTRL_COL_W: f32 = 232.0;

#[derive(Clone, Copy)]
pub(super) enum RowLayout {
    Standard,
    IntrinsicControl,
}

/// One heading role for settings groups, including specialized feature pages.
pub(super) fn group_heading(
    title: impl Into<SharedString>,
    item_font_size: f32,
    cx: &App,
) -> gpui::Div {
    div()
        .text_size(px(item_font_size * GROUP_TITLE_SCALE))
        .font_weight(FontWeight::MEDIUM)
        .text_color(cx.theme().foreground)
        .child(title.into())
}

impl SettingsPane {
    pub(super) fn row_padding_y(&self) -> f32 {
        if self.runtime.density == pebrel_settings::DensityName::Compact {
            ROW_PAD_Y_COMPACT
        } else {
            ROW_PAD_Y
        }
    }

    /// 一组设置：标题在卡外，行在卡内。返回的构建器照常 `.child(row)`。
    pub(crate) fn group(&self, title: &'static str, cx: &Context<Self>) -> SettingsGroup {
        SettingsGroup { title, rows: Vec::new(), base_px: self.font_size_px(cx) }
    }

    /// 组与组之间的间隔。
    ///
    pub(crate) fn group_divider(_cx: &Context<Self>) -> gpui::Div {
        div().w_full().h(px(GROUP_GAP - 8.0)).flex_shrink_0()
    }

    /// 设置行。`desc` 写后果；确实无后果可说的项才传空串（尽量不要有）。
    pub(crate) fn row(
        &self,
        label: &'static str,
        desc: impl Into<SettingHelp>,
        control: impl IntoElement,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        self.row_shell(label, desc.into(), None, RowLayout::Standard, control, cx)
    }

    /// 偏离默认值的设置项常驻显示恢复按钮，恢复后移除。
    pub(crate) fn row_with_reset(
        &self,
        label: &'static str,
        desc: impl Into<SettingHelp>,
        dirty: bool,
        on_reset: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        control: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        self.row_with_reset_layout(label, desc, dirty, on_reset, RowLayout::Standard, control, cx)
    }

    pub(super) fn row_with_reset_layout(
        &self,
        label: &'static str,
        desc: impl Into<SettingHelp>,
        dirty: bool,
        on_reset: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        layout: RowLayout,
        control: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let reset = dirty.then(|| {
            let action = crate::gpui_shell::config::ui_language(cx)
                .text(crate::i18n::Message::SettingsPaneRestoreDefault);
            let id: SharedString = format!("setting-reset-{label}").into();
            let ink = cx.theme().foreground;
            let hover = crate::gpui_shell::theme::neutral_hover(cx);
            Button::new(id.clone())
                .debug_selector(move || format!("setting-reset-{label}"))
                .group(id.clone())
                .custom(ButtonCustomVariant::new(cx).foreground(cx.theme().muted_foreground))
                .size(px(32.0))
                .px_0()
                .rounded(px(6.0))
                .cursor_pointer()
                .aria_label(action)
                .tooltip(action)
                .child(
                    h_flex()
                        .id("reset-face")
                        .debug_selector(move || format!("setting-reset-face-{label}"))
                        .w(px(30.0))
                        .h(px(26.0))
                        .justify_center()
                        .rounded(px(6.0))
                        .group_hover(id, move |style| style.bg(hover).text_color(ink))
                        .active(move |style| style.bg(ink.opacity(0.12)))
                        .child(Icon::new(IconName::Undo2).size(px(12.0))),
                )
                .on_click(cx.listener(move |this, _, window, cx| on_reset(this, window, cx)))
                .into_any_element()
        });
        self.row_shell(label, desc.into(), reset, layout, control, cx)
    }

    /// 行内文字共用中性的悬停反馈。
    fn row_hover_group(label: &'static str) -> SharedString {
        SharedString::from(format!("settings-row-{label}"))
    }

    /// 说明文字。反引号包起来的片段走等宽——这是全页 mono 唯一的用途。
    ///
    /// 我们是终端，所以等宽在界面里不能当默认字体用，否则它什么也没说。
    /// 让它只出现在路径、键帽、配置键名这类**机器读的字面量**上，它才变成
    /// 一个信号：这几个字符是可以照抄的，一个也不能改。
    ///
    /// 直接拼 `div` 做不到这件事——那样每段会各占一个 flex 盒子，一句话被
    /// 切成几块、还断不了行。所以走 `StyledText` 的 run：同一次排版里换字体，
    /// 换行照旧。
    fn desc_text(desc: &'static str, cx: &Context<Self>) -> gpui::AnyElement {
        if !desc.contains('`') {
            return desc.into_any_element();
        }
        let theme = cx.theme();
        let sans = crate::gpui_shell::typography::ui_font(cx);
        let mono = gpui::font(theme.mono_font_family.clone());
        let (dim, ink) = (theme.muted_foreground, theme.foreground);

        let mut text = String::with_capacity(desc.len());
        let mut runs: Vec<gpui::TextRun> = Vec::new();
        // 奇数段在反引号内。反引号本身不进最终文本，它只是标记。
        for (ix, piece) in desc.split('`').enumerate() {
            if piece.is_empty() {
                continue;
            }
            let literal = ix % 2 == 1;
            text.push_str(piece);
            runs.push(gpui::TextRun {
                len: piece.len(),
                font: if literal { mono.clone() } else { sans.clone() },
                // 字面量比周围说明亮一档：字体已经把它分出来了，颜色再补一点
                // 权重，扫视时才不至于滑过去。
                color: if literal { ink } else { dim },
                background_color: None,
                underline: None,
                strikethrough: None,
            });
        }
        gpui::StyledText::new(text).with_runs(runs).into_any_element()
    }

    pub(super) fn row_shell(
        &self,
        label: &'static str,
        desc: SettingHelp,
        reset: Option<gpui::AnyElement>,
        layout: RowLayout,
        control: impl IntoElement,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let base_px = self.font_size_px(cx);
        let expanded = self.expanded_setting_help.contains(label);
        let pad_y = self.row_padding_y();
        let hover_group = Self::row_hover_group(label);
        let mut title_hover = theme.foreground;
        title_hover.l =
            (title_hover.l + if theme.is_dark() { 0.035 } else { -0.035 }).clamp(0.0, 1.0);
        let detail_hover = theme.muted_foreground.blend(theme.foreground.opacity(0.18));
        let text = v_flex()
            .child(
                h_flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .min_w_0()
                            .text_size(px(base_px))
                            .font_weight(FontWeight::NORMAL)
                            .text_color(theme.foreground)
                            .group_hover(hover_group.clone(), move |text| {
                                text.text_color(title_hover)
                            })
                            .child(label),
                    )
                    .when(desc.details.is_some(), |heading| {
                        let language = crate::gpui_shell::config::ui_language(cx);
                        let action = if expanded {
                            language.text(crate::i18n::Message::SettingsPaneHideDetails)
                        } else {
                            language.text(crate::i18n::Message::SettingsPaneMoreDetails)
                        };
                        heading.child(
                            Button::new(SharedString::from(format!("settings-help-{label}")))
                                .icon(IconName::Info)
                                .ghost()
                                .size(px(32.0))
                                .text_color(theme.muted_foreground)
                                .accessibility_id(SharedString::from(format!(
                                    "settings-help-{label}"
                                )))
                                .toggled(expanded)
                                .tooltip(format!("{label}: {action}"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if !this.expanded_setting_help.insert(label) {
                                        this.expanded_setting_help.remove(label);
                                    }
                                    cx.notify();
                                })),
                        )
                    })
                    .children(reset),
            )
            .when(!desc.summary.is_empty(), |text| {
                text.child(
                    div()
                        .mt(px(LABEL_DESC_GAP))
                        .text_size(px(base_px * DESC_SCALE))
                        .font_weight(FontWeight::NORMAL)
                        .text_color(theme.muted_foreground)
                        .group_hover(hover_group.clone(), move |text| text.text_color(detail_hover))
                        .child(Self::desc_text(desc.summary, cx)),
                )
            })
            .when_some(desc.details.filter(|_| expanded), |text, details| {
                text.child(
                    div()
                        .mt(px(8.0))
                        .text_size(px(base_px * DESC_SCALE))
                        .text_color(theme.muted_foreground)
                        .child(Self::desc_text(details, cx)),
                )
            });
        let control = control.into_any_element();
        let columns = match layout {
            RowLayout::IntrinsicControl => {
                // 分段条依实际文字取宽；空间不足时整组换行。
                h_flex()
                    .w_full()
                    .items_center()
                    .flex_wrap()
                    .gap_4()
                    .child(text.flex_1().min_w(px(180.0)))
                    .child(h_flex().flex_grow(1.0).justify_end().max_w_full().child(control))
            },
            RowLayout::Standard => h_flex()
                .w_full()
                .items_center()
                .flex_wrap()
                .gap_4()
                .child(text.flex_1().min_w(px(180.0)))
                .child(
                    h_flex()
                        .w(px(CTRL_COL_W))
                        .max_w_full()
                        .flex_shrink_0()
                        .justify_end()
                        .items_center()
                        .child(control),
                ),
        };
        div()
            .id(label)
            .debug_selector(move || format!("settings-row-{label}"))
            .group(Self::row_hover_group(label))
            .relative()
            .w_full()
            .flex_shrink_0()
            .px(px(CARD_PAD_X))
            .py(px(pad_y))
            .child(columns)
    }
}

pub(super) fn settings_card(cx: &App) -> gpui::Div {
    v_flex()
        .w_full()
        .min_w_0()
        .bg(crate::gpui_shell::theme::settings_card_fill(cx))
        .rounded(px(CARD_RADIUS))
        .shadow(crate::gpui_shell::theme::settings_card_shadow(cx))
}

/// 设置分组卡。`ParentElement` 把子元素收进卡内，调用点保持
/// `self.group(title, cx).child(row)` 的写法；行之间插内缩细线。
#[derive(IntoElement)]
pub(crate) struct SettingsGroup {
    title: &'static str,
    rows: Vec<gpui::AnyElement>,
    base_px: f32,
}

impl gpui::ParentElement for SettingsGroup {
    fn extend(&mut self, elements: impl IntoIterator<Item = gpui::AnyElement>) {
        self.rows.extend(elements);
    }
}

impl gpui::RenderOnce for SettingsGroup {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let divider = crate::gpui_shell::theme::settings_hairline(cx);
        let title = self.title;
        let mut card =
            settings_card(cx).debug_selector(move || format!("settings-group-card-{title}"));
        for (ix, row) in self.rows.into_iter().enumerate() {
            if ix > 0 {
                card = card.child(div().h(px(1.0)).mx(px(CARD_PAD_X)).flex_shrink_0().bg(divider));
            }
            card = card.child(row);
        }
        let theme = cx.theme();
        v_flex()
            .w_full()
            .child(
                div()
                    .pl(px(4.0))
                    .pb(px(8.0))
                    .text_size(px(self.base_px * CARD_TITLE_SCALE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.muted_foreground.blend(theme.foreground.opacity(0.55)))
                    .child(title),
            )
            .child(card)
    }
}
