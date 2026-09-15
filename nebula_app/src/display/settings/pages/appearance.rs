// Settings UI: Appearance page (theme, colors, cursor, interface, terminal presentation).

use crate::display::caret_blink_on;
use crate::display::color::Rgb;
use crate::display::ui::theme::Skin;
use crate::display::ui::{tokens, widgets};
use crate::display::{
    contains_rect, truncate_tab_label, NebulaSettingsSection, NebulaTheme, SettingsHit, SizeInfo,
    UiLanguage,
};
use crate::display::settings::geometry::SettingsGeometry;
use crate::display::settings::view::{preview_line_y, SettingsView};
use crate::display::settings::{
    background_image_alignment_label, background_image_fit_label, cell_width_mode_label,
    cursor_shape_label, density_label, format_hex_rgb, language_label, SettingsDropdown,
    SettingsOpacityTarget, PREVIEW_PROMPT_COLS,
};
use crate::display::settings::render::{
    draw_big_text, draw_button_label, keymap_group_title, proxy_section_title_y, row_label,
    row_label_with_right_inset, section_title, warning_lines,
};
use crate::renderer::ui::{Rgba, UiQuad};
use crate::renderer::{GlyphCache, Renderer};
use nebula_terminal::vte::ansi::CursorShape;
use unicode_width::UnicodeWidthChar;

pub(crate) fn push_appearance_quads(
    view: &SettingsView,
    quads: &mut Vec<UiQuad>,
    size: &SizeInfo,
    scale: f32,
    geometry: &SettingsGeometry,
    sk: &Skin,
    clip_top: f32,
    clip_bot: f32,
) {
    let s = |v: f32| v * scale;
    let clip = |quads: &mut Vec<UiQuad>, quad: UiQuad| {
        if let Some(quad) = quad.clip_y(clip_top, clip_bot) {
            quads.push(quad);
        }
    };
    let mut staged: Vec<UiQuad> = Vec::new();
    let group_frame = |_quads: &mut Vec<UiQuad>, _first_row, _rows: usize| {};
    let row_hover = |_quads: &mut Vec<UiQuad>, _rect, _hovered: bool| {};
    let action_button = |quads: &mut Vec<UiQuad>, row, logical_w: f32, hovered: bool| {
        let rect = super::super::row_action_rect(row, scale, logical_w);
        clip(
            quads,
            UiQuad::solid(
                rect.0,
                rect.1,
                rect.2,
                rect.3,
                rect.3 * 0.5,
                if hovered { sk.hover } else { sk.surface },
            ),
        );
    };
    let combobox = |quads: &mut Vec<UiQuad>,
                    staged: &mut Vec<UiQuad>,
                    row,
                    hot: bool,
                    open: bool| {
        widgets::push_combobox(staged, widgets::combobox_rect(row, scale), scale, &sk, hot, open);
        for quad in staged.drain(..) {
            clip(quads, quad);
        }
    };
    let slider = |quads: &mut Vec<UiQuad>, staged: &mut Vec<UiQuad>, hit, value: f32, hot: bool| {
        widgets::push_slider(staged, hit, value, scale, &sk, hot);
        for quad in staged.drain(..) {
            clip(quads, quad);
        }
    };
    let toggle = |quads: &mut Vec<UiQuad>,
                  staged: &mut Vec<UiQuad>,
                  row,
                  hit: SettingsHit,
                  on: bool,
                  _hot: bool,
                  _pressed: bool| {
        let motion = super::super::settings_toggle_slot(hit)
            .map_or_else(|| widgets::ToggleMotion::settled(on), |index| view.toggle_motion[index]);
        widgets::push_toggle(staged, row, scale, &sk, motion);
        for quad in staged.drain(..) {
            clip(quads, quad);
        }
    };
            // ---- Live preview card: configure → immediately see ----
            // Terminal colors, font family/size (text pass), and the demo
            // cursor all read the same state the real grid uses.
            {
                let (vx, vy, vw, vh) = geometry.preview;
                clip(
                    quads,
                    UiQuad::solid(
                        vx - s(1.0),
                        vy - s(1.0),
                        vw + s(2.0),
                        vh + s(2.0),
                        s(11.0),
                        sk.hairline,
                    ),
                );
                clip(
                    quads,
                    UiQuad::solid(
                        vx,
                        vy,
                        vw,
                        vh,
                        s(10.0),
                        Rgba::new(view.preview_bg.r, view.preview_bg.g, view.preview_bg.b, 255),
                    ),
                );
                // Demo cursor on the prompt line, driven by the REAL shape +
                // blink settings (shares the UI caret's 500ms phase).
                if !view.cursor_blink || caret_blink_on() {
                    let cell_w = size.cell_width();
                    let cell_h = size.cell_height();
                    let cursor_x = vx + s(16.0) + PREVIEW_PROMPT_COLS as f32 * cell_w;
                    let cursor_y = preview_line_y(vy, cell_h, 2.0, scale);
                    let ink =
                        Rgba::new(view.preview_fg.r, view.preview_fg.g, view.preview_fg.b, 235);
                    let bg =
                        Rgba::new(view.preview_bg.r, view.preview_bg.g, view.preview_bg.b, 255);
                    let stroke = (1.5 * scale).max(1.0);
                    let beam_w = (2.0 * scale).max(1.0);
                    match view.cursor_shape {
                        CursorShape::Beam => {
                            clip(
                                quads,
                                UiQuad::solid(cursor_x, cursor_y, beam_w, cell_h, 0.0, ink),
                            );
                        },
                        CursorShape::Underline => {
                            clip(
                                quads,
                                UiQuad::solid(
                                    cursor_x,
                                    cursor_y + cell_h - beam_w,
                                    cell_w,
                                    beam_w,
                                    0.0,
                                    ink,
                                ),
                            );
                        },
                        CursorShape::HollowBlock => {
                            clip(
                                quads,
                                UiQuad::solid(cursor_x, cursor_y, cell_w, cell_h, 0.0, ink),
                            );
                            clip(
                                quads,
                                UiQuad::solid(
                                    cursor_x + stroke,
                                    cursor_y + stroke,
                                    cell_w - 2.0 * stroke,
                                    cell_h - 2.0 * stroke,
                                    0.0,
                                    bg,
                                ),
                            );
                        },
                        CursorShape::Hidden => {},
                        CursorShape::Block => {
                            clip(
                                quads,
                                UiQuad::solid(cursor_x, cursor_y, cell_w, cell_h, 0.0, ink),
                            );
                        },
                    }
                }
            }

            // Theme cards are MINIATURE TERMINAL WINDOWS, each painted in its
            // own theme's colors: shell_bg window shell, a rounded term_bg
            // "terminal card" floating inside it (the real window's model,
            // shrunk), and fake prompt/output lines in the theme's own inks.
            // A flat panel swatch only answered "what color is the chrome";
            // the mini window answers what the picker is really asked: how do
            // background, text and highlights look TOGETHER. (2026-07-28 用户
            // 裁定；窗控红绿灯明确不画——各平台窗控样式不同，预览不预设
            // 任何一家。) Selection = accent ring + halo; hover = 2px lift —
            // no wash, so the preview colors stay true.
            for (theme, ox, oy, ow, oh) in geometry.options {
                let selected = theme == view.theme;
                let hovered = view.hover == SettingsHit::Theme(theme);
                let lift = if hovered && !selected { s(2.0) } else { 0.0 };
                let oy = oy - lift;
                let stroke = if selected {
                    Rgba::new(sk.accent.r, sk.accent.g, sk.accent.b, 255)
                } else {
                    sk.hairline
                };
                let stroke_w = if selected { s(2.0) } else { s(1.0) };
                if selected {
                    // Selected card glows softly: the accent ring plus a
                    // diffuse halo, per the design sheet's lit-control look.
                    clip(
                        quads,
                        UiQuad::glow(
                            ox - s(14.0),
                            oy - s(14.0),
                            ow + s(28.0),
                            oh + s(28.0),
                            Rgba::new(sk.accent.r, sk.accent.g, sk.accent.b, 66),
                        ),
                    );
                } else if hovered {
                    // Hover halo: same shape, fainter — enough 辉光 to read
                    // as "lit up" without competing with the selected card.
                    clip(
                        quads,
                        UiQuad::glow(
                            ox - s(12.0),
                            oy - s(10.0),
                            ow + s(24.0),
                            oh + s(26.0),
                            Rgba::new(sk.accent.r, sk.accent.g, sk.accent.b, 38),
                        ),
                    );
                }
                clip(
                    quads,
                    UiQuad::solid(
                        ox - stroke_w,
                        oy - stroke_w,
                        ow + 2.0 * stroke_w,
                        oh + 2.0 * stroke_w,
                        s(9.0),
                        stroke,
                    ),
                );
                let p = theme.palette();
                let ink = theme.card_ink();
                let shell = Rgba::new(p.shell_bg.r, p.shell_bg.g, p.shell_bg.b, 255);
                clip(quads, UiQuad::solid(ox, oy, ow, oh, s(8.0), shell));
                // Inner terminal card. The taller top margin reads as a title
                // bar without drawing one.
                let (tx, ty) = (ox + s(10.0), oy + s(14.0));
                let (tw, th) = (ow - s(20.0), oh - s(22.0));
                let term = Rgba::new(p.term_bg.r, p.term_bg.g, p.term_bg.b, 255);
                clip(quads, UiQuad::solid(tx, ty, tw, th, s(5.0), term));
                // Three fake lines as pill bars: a prompt command in fg (the
                // `❯` itself is a real glyph, drawn in the text pass), two
                // highlight tokens, a dim trailing line. Widths are fractions
                // of the card so narrow cards keep the proportions.
                let bar_h = s(3.0);
                let line_x = tx + s(8.0);
                let line_pitch = s(11.0);
                let y0 = ty + s(8.0);
                let inner_w = tw - s(16.0);
                let bar = |x: f32, y: f32, w: f32, c: Rgb, a: u8| {
                    UiQuad::solid(x, y, w, bar_h, s(1.5), Rgba::new(c.r, c.g, c.b, a))
                };
                let prompt_w = s(9.0); // room the text-pass ❯ occupies
                let cmd_w = inner_w * 0.42;
                clip(quads, bar(line_x + prompt_w, y0, cmd_w, ink.fg, 230));
                // A block caret hugging the command's end, in the theme's
                // accent — the one "alive" spark on the card.
                let acc = theme.accent();
                clip(
                    quads,
                    UiQuad::solid(
                        line_x + prompt_w + cmd_w + s(3.0),
                        y0 - s(2.0),
                        s(3.0),
                        bar_h + s(4.0),
                        s(1.0),
                        Rgba::new(acc.r, acc.g, acc.b, 255),
                    ),
                );
                // 第二行是各主题的品牌双色（edge_l→edge_r，即侧栏品牌
                // 渐变对）：固定 ANSI green/blue 让 4 张暗卡 3 张亮卡两两
                // 同色（2026-07-28 用户反馈「预览颜色都一样」），身份色带
                // 才是卡片间唯一稳定的区分资产。
                clip(
                    quads,
                    bar(
                        line_x,
                        y0 + line_pitch,
                        inner_w * 0.28,
                        Rgb::new(p.edge_l.r, p.edge_l.g, p.edge_l.b),
                        235,
                    ),
                );
                clip(
                    quads,
                    bar(
                        line_x + inner_w * 0.28 + s(5.0),
                        y0 + line_pitch,
                        inner_w * 0.20,
                        Rgb::new(p.edge_r.r, p.edge_r.g, p.edge_r.b),
                        235,
                    ),
                );
                clip(quads, bar(line_x, y0 + 2.0 * line_pitch, inner_w * 0.55, ink.fg, 96));
            }

            group_frame(quads, geometry.system_theme, 1);
            row_hover(quads, geometry.system_theme, view.hover == SettingsHit::SystemThemeToggle);
            toggle(
                quads,
                &mut staged,
                geometry.system_theme,
                SettingsHit::SystemThemeToggle,
                view.follow_system_theme,
                view.hover == SettingsHit::SystemThemeToggle,
                view.pressed == SettingsHit::SystemThemeToggle,
            );

            // 自定义背景和界面都使用连续分组，避免设置 Tab 内再次出现
            // 漂浮卡片语言。
            group_frame(quads, geometry.background, 6);
            row_hover(quads, geometry.background, view.hover == SettingsHit::BackgroundColor);
            // 背景色也是多选项设置：同一 combobox 组件，浮层换成色板+hex。
            combobox(
                quads,
                &mut staged,
                geometry.background,
                view.hover == SettingsHit::BackgroundColor,
                view.dropdown == Some(SettingsDropdown::BackgroundColor),
            );
            row_hover(quads, geometry.background_image, view.hover == SettingsHit::BackgroundImage);
            if view.background_image.is_some() {
                row_hover(
                    quads,
                    geometry.background_image_clear,
                    view.hover == SettingsHit::BackgroundImageClear,
                );
            }
            combobox(
                quads,
                &mut staged,
                geometry.background_image_fit,
                view.hover == SettingsHit::BackgroundImageFit,
                view.dropdown == Some(SettingsDropdown::BackgroundFit),
            );
            combobox(
                quads,
                &mut staged,
                geometry.background_image_alignment,
                view.hover == SettingsHit::BackgroundImageAlignment,
                view.dropdown == Some(SettingsDropdown::BackgroundAlignment),
            );
            slider(
                quads,
                &mut staged,
                geometry.background_image_opacity_slider,
                view.background_image_opacity,
                view.hover == SettingsHit::BackgroundImageOpacitySlider
                    || view.dragging_opacity == Some(SettingsOpacityTarget::BackgroundImage),
            );
            row_hover(
                quads,
                geometry.background_image_cover_chrome,
                view.hover == SettingsHit::BackgroundImageCoverChrome,
            );
            toggle(
                quads,
                &mut staged,
                geometry.background_image_cover_chrome,
                SettingsHit::BackgroundImageCoverChrome,
                view.background_image_cover_chrome,
                view.hover == SettingsHit::BackgroundImageCoverChrome,
                view.pressed == SettingsHit::BackgroundImageCoverChrome,
            );

            // 光标组：形状下拉 + 闪烁开关。
            group_frame(quads, geometry.cursor_shape_row, 2);
            row_hover(
                quads,
                geometry.cursor_shape_row,
                view.hover == SettingsHit::CursorShapeDropdown,
            );
            combobox(
                quads,
                &mut staged,
                geometry.cursor_shape_row,
                view.hover == SettingsHit::CursorShapeDropdown,
                view.dropdown == Some(SettingsDropdown::CursorShape),
            );
            row_hover(
                quads,
                geometry.cursor_blink_row,
                view.hover == SettingsHit::CursorBlinkToggle,
            );
            toggle(
                quads,
                &mut staged,
                geometry.cursor_blink_row,
                SettingsHit::CursorBlinkToggle,
                view.cursor_blink,
                view.hover == SettingsHit::CursorBlinkToggle,
                view.pressed == SettingsHit::CursorBlinkToggle,
            );

            // 界面组：语言（同一通用下拉组件）+ 界面外观预设 + 终端不透明度
            // + 背景模糊。
            group_frame(quads, geometry.language_row, 3);
            row_hover(quads, geometry.language_row, view.hover == SettingsHit::LanguageDropdown);
            combobox(
                quads,
                &mut staged,
                geometry.language_row,
                view.hover == SettingsHit::LanguageDropdown,
                view.dropdown == Some(SettingsDropdown::Language),
            );
            row_hover(quads, geometry.density_row, view.hover == SettingsHit::DensityDropdown);
            combobox(
                quads,
                &mut staged,
                geometry.density_row,
                view.hover == SettingsHit::DensityDropdown,
                view.dropdown == Some(SettingsDropdown::Density),
            );
            slider(
                quads,
                &mut staged,
                geometry.opacity_slider,
                view.opacity,
                view.hover == SettingsHit::OpacitySlider
                    || view.dragging_opacity == Some(SettingsOpacityTarget::Terminal),
            );
            row_hover(quads, geometry.blur, view.hover == SettingsHit::BlurToggle);
            toggle(
                quads,
                &mut staged,
                geometry.blur,
                SettingsHit::BlurToggle,
                view.blur,
                view.hover == SettingsHit::BlurToggle,
                view.pressed == SettingsHit::BlurToggle,
            );

            group_frame(quads, geometry.font_size_row, 4);
            widgets::push_spinner(
                &mut staged,
                geometry.font_size_row,
                scale,
                &sk,
                view.hover == SettingsHit::FontSizeUp,
                view.hover == SettingsHit::FontSizeDown,
            );
            row_hover(
                quads,
                geometry.cell_width_mode,
                view.hover == SettingsHit::CellWidthModeDropdown,
            );
            combobox(
                quads,
                &mut staged,
                geometry.cell_width_mode,
                view.hover == SettingsHit::CellWidthModeDropdown,
                view.dropdown == Some(SettingsDropdown::CellWidthMode),
            );
            for quad in staged.drain(..) {
                clip(quads, quad);
            }
            row_hover(quads, geometry.fetch, view.hover == SettingsHit::FetchToggle);
            row_hover(quads, geometry.powerline, view.hover == SettingsHit::PowerlineToggle);
            toggle(
                quads,
                &mut staged,
                geometry.fetch,
                SettingsHit::FetchToggle,
                view.fetch,
                view.hover == SettingsHit::FetchToggle,
                view.pressed == SettingsHit::FetchToggle,
            );
            toggle(
                quads,
                &mut staged,
                geometry.powerline,
                SettingsHit::PowerlineToggle,
                view.powerline,
                view.hover == SettingsHit::PowerlineToggle,
                view.pressed == SettingsHit::PowerlineToggle,
            );
    quads.extend(staged.drain(..));
}

pub(crate) fn draw_appearance_text(
    view: &SettingsView,
    r: &mut Renderer,
    gc: &mut GlyphCache,
    size: &SizeInfo,
    scale: f32,
    geometry: &SettingsGeometry,
    sk: &Skin,
    language: UiLanguage,
    cell_w: f32,
    cell_h: f32,
    icon_draws: &mut Vec<(String, (f32, f32, f32, f32))>,
    content_x: f32,
    px: f32,
    clip_top: f32,
    clip_bot: f32,
    title_h: f32,
) {
    let s = |v: f32| v * scale;
    let visible = |ry: f32, rh: f32| ry >= clip_top && ry + rh <= clip_bot;
    let row_text_y = |ry: f32, rh: f32| {
        if geometry.stacked_rows { ry + s(9.0) } else { ry + (rh - cell_h) / 2.0 }
    };
    let combobox_value = |r: &mut Renderer,
                          gc: &mut GlyphCache,
                          row: (f32, f32, f32, f32),
                          value: &str,
                          ink: Rgb| {
        let rect = widgets::combobox_rect(row, scale);
        let tx = widgets::combobox_text_x(rect, scale);
        let right = widgets::combobox_text_right(rect, scale);
        let max_chars = ((right - tx).max(cell_w) / cell_w).floor().max(1.0) as usize;
        let value = truncate_tab_label(value, max_chars);
        r.draw_chrome_text(size, tx, rect.1 + (rect.3 - cell_h) / 2.0, ink, &value, gc);
    };
    let combobox_value_rect = |r: &mut Renderer,
                               gc: &mut GlyphCache,
                               rect: (f32, f32, f32, f32),
                               value: &str,
                               ink: Rgb| {
        let tx = widgets::combobox_text_x(rect, scale);
        let right = widgets::combobox_text_right(rect, scale);
        let max_chars = ((right - tx).max(cell_w) / cell_w).floor().max(1.0) as usize;
        let value = truncate_tab_label(value, max_chars);
        r.draw_chrome_text(size, tx, rect.1 + (rect.3 - cell_h) / 2.0, ink, &value, gc);
    };
    let group_y = |row_y: f32| row_y - s(42.0);
            // Live preview: sample lines in the CURRENT font/size on the
            // CURRENT terminal colors; the demo cursor quad shares this
            // layout via `preview_line_y`.
            {
                let (vx, vy, _, vh) = geometry.preview;
                if visible(group_y(vy), title_h) {
                    section_title(
                        r,
                        gc,
                        size,
                        scale,
                        &sk,
                        content_x + s(24.0),
                        group_y(vy),
                        language.pick("预览", "Preview"),
                    );
                }
                if visible(vy, vh) {
                    let fg = view.preview_fg;
                    r.draw_chrome_text(
                        size,
                        vx + s(16.0),
                        preview_line_y(vy, cell_h, 0.0, scale),
                        fg,
                        "user@nebula ~ $ nebula --version",
                        gc,
                    );
                    let sample = format!(
                        "Nebula Terminal · {} · {:.0}px",
                        view.font_family, view.font_size_px
                    );
                    r.draw_chrome_text(
                        size,
                        vx + s(16.0),
                        preview_line_y(vy, cell_h, 1.0, scale),
                        fg,
                        &sample,
                        gc,
                    );
                    r.draw_chrome_text(
                        size,
                        vx + s(16.0),
                        preview_line_y(vy, cell_h, 2.0, scale),
                        fg,
                        "❯",
                        gc,
                    );
                }
            }
            let cards_y = geometry.options[0].2;
            if visible(group_y(cards_y), title_h) {
                section_title(
                    r,
                    gc,
                    size,
                    scale,
                    &sk,
                    content_x + s(24.0),
                    group_y(cards_y),
                    language.pick("主题", "Themes"),
                );
            }
            for (theme, ox, oy, ow, oh) in geometry.options {
                let selected = theme == view.theme;
                let hovered = view.hover == SettingsHit::Theme(theme);
                // The label rides the card's 2px hover lift (quads do the
                // same), and hides only when IT would cross the viewport edge
                // — a half-clipped card keeps its fully-visible label.
                let lift = if hovered && !selected { s(2.0) } else { 0.0 };
                // The mini window's prompt glyph, in the card's own accent —
                // the quads pass carries the fake-output bars, this is the one
                // real glyph. draw_ui_text rasterizes at the true tiny size
                // (GPU-stretched atlas bitmaps would go fuzzy), and its cell
                // top is placed so the glyph's midline meets the command bar's.
                let prompt_y = oy + s(18.0) - lift;
                if visible(prompt_y, cell_h) {
                    r.draw_ui_text(
                        size,
                        ox + s(18.0),
                        prompt_y,
                        0.55,
                        theme.accent(),
                        nebula_terminal::term::cell::Flags::BOLD,
                        "❯",
                        gc,
                    );
                }
                let text_y = oy + oh + s(12.0) - lift;
                if !visible(text_y, cell_h) {
                    continue;
                }
                let card_label = theme.short_label();
                r.draw_chrome_text(
                    size,
                    ox + (ow - card_label.chars().count() as f32 * cell_w) / 2.0,
                    text_y,
                    if selected {
                        sk.accent
                    } else if hovered {
                        sk.ink
                    } else {
                        sk.ink_dim
                    },
                    card_label,
                    gc,
                );
            }
            let (st_x, st_y, _, st_h) = geometry.system_theme;
            if visible(group_y(st_y), title_h) {
                section_title(
                    r,
                    gc,
                    size,
                    scale,
                    &sk,
                    st_x,
                    group_y(st_y),
                    language.pick("主题模式", "Theme mode"),
                );
            }
            if visible(st_y, st_h) {
                r.draw_chrome_text(
                    size,
                    st_x + s(16.0),
                    row_text_y(st_y, st_h),
                    sk.ink,
                    language.pick("跟随系统明暗模式", "Follow system appearance"),
                    gc,
                );
            }
            let (bg_x, bg_y, _, bg_h) = geometry.background;
            if visible(group_y(bg_y), title_h) {
                section_title(
                    r,
                    gc,
                    size,
                    scale,
                    &sk,
                    bg_x,
                    group_y(bg_y),
                    language.pick("自定义背景", "Custom background"),
                );
            }
            if visible(bg_y, bg_h) {
                let background_v = view
                    .background
                    .map(format_hex_rgb)
                    .unwrap_or_else(|| language.pick("主题默认", "Theme default").to_owned());
                row_label(
                    r,
                    gc,
                    size,
                    scale,
                    &sk,
                    geometry.background,
                    language.pick("背景色", "Background color"),
                    "",
                    sk.accent,
                );
                // 值画进 combobox 控件框内（chevron 井之前），右对齐到行缘
                // 会压住下拉箭头（浅色模式下重叠尤其明显）。
                combobox_value(r, gc, geometry.background, &background_v, sk.accent);
            }
            let (img_x, img_y, _, img_h) = geometry.background_image;
            let _ = img_x;
            if visible(img_y, img_h) {
                let image_v = view
                    .background_image
                    .as_deref()
                    .map(str::to_owned)
                    .unwrap_or_else(|| language.pick("未设置", "Not set").to_owned());
                row_label_with_right_inset(
                    r,
                    gc,
                    size,
                    scale,
                    &sk,
                    geometry.background_image,
                    language.pick("背景图片", "Background image"),
                    &image_v,
                    sk.accent,
                    if view.background_image.is_some() { s(48.0) } else { 0.0 },
                );
                if view.background_image.is_some() {
                    let (cx, cy, cw, ch) = geometry.background_image_clear;
                    r.draw_chrome_text(
                        size,
                        cx + (cw - cell_w) / 2.0,
                        cy + (ch - cell_h) / 2.0,
                        if view.hover == SettingsHit::BackgroundImageClear {
                            sk.ink
                        } else {
                            sk.ink_dim
                        },
                        "↶",
                        gc,
                    );
                }
            }
            let (_, fit_y, _, fit_h) = geometry.background_image_fit;
            if visible(fit_y, fit_h) {
                row_label(
                    r,
                    gc,
                    size,
                    scale,
                    &sk,
                    geometry.background_image_fit,
                    language.pick("背景图像拉伸模式", "Background image stretch mode"),
                    "",
                    sk.ink,
                );
                combobox_value(
                    r,
                    gc,
                    geometry.background_image_fit,
                    background_image_fit_label(view.background_image_fit, language),
                    sk.accent,
                );
            }
            let (_, align_y, _, align_h) = geometry.background_image_alignment;
            if visible(align_y, align_h) {
                row_label(
                    r,
                    gc,
                    size,
                    scale,
                    &sk,
                    geometry.background_image_alignment,
                    language.pick("背景图像对齐", "Background image alignment"),
                    "",
                    sk.ink,
                );
                combobox_value(
                    r,
                    gc,
                    geometry.background_image_alignment,
                    background_image_alignment_label(view.background_image_alignment, language),
                    sk.accent,
                );
            }
            let (_, image_opacity_y, _, image_opacity_h) = geometry.background_image_opacity_row;
            if visible(image_opacity_y, image_opacity_h) {
                let stacked = image_opacity_h >= s(56.0);
                let text_y = if stacked {
                    image_opacity_y + s(9.0)
                } else {
                    image_opacity_y + (image_opacity_h - cell_h) / 2.0
                };
                r.draw_chrome_text(
                    size,
                    geometry.background_image_opacity_row.0 + s(16.0),
                    text_y,
                    sk.ink,
                    language.pick("背景图像不透明度", "Background image opacity"),
                    gc,
                );
                let image_opacity_v = format!("{:.0}%", view.background_image_opacity * 100.0);
                let image_opacity_cols: usize =
                    image_opacity_v.chars().map(|c| c.width().unwrap_or(0)).sum();
                r.draw_chrome_text(
                    size,
                    if stacked {
                        geometry.background_image_opacity_row.0
                            + geometry.background_image_opacity_row.2
                            - s(16.0)
                            - image_opacity_cols as f32 * cell_w
                    } else {
                        geometry.background_image_opacity_slider.0
                            - s(10.0)
                            - image_opacity_cols as f32 * cell_w
                    },
                    text_y,
                    sk.accent,
                    &image_opacity_v,
                    gc,
                );
            }
            let (_, cover_y, _, cover_h) = geometry.background_image_cover_chrome;
            if visible(cover_y, cover_h) {
                row_label(
                    r,
                    gc,
                    size,
                    scale,
                    &sk,
                    geometry.background_image_cover_chrome,
                    language.pick(
                        "将背景图扩展到标题栏和侧边栏",
                        "Extend background image into title bar and sidebar",
                    ),
                    "",
                    sk.ink,
                );
            }
            // ---- 光标组 ----
            let (cs_x, cs_y, _, cs_h) = geometry.cursor_shape_row;
            if visible(group_y(cs_y), title_h) {
                section_title(
                    r,
                    gc,
                    size,
                    scale,
                    &sk,
                    cs_x,
                    group_y(cs_y),
                    language.pick("光标", "Cursor"),
                );
            }
            if visible(cs_y, cs_h) {
                row_label(
                    r,
                    gc,
                    size,
                    scale,
                    &sk,
                    geometry.cursor_shape_row,
                    language.pick("光标形状", "Cursor shape"),
                    "",
                    sk.ink,
                );
                combobox_value(
                    r,
                    gc,
                    geometry.cursor_shape_row,
                    cursor_shape_label(view.cursor_shape, language),
                    sk.accent,
                );
            }
            let (_, blink_y, _, blink_h) = geometry.cursor_blink_row;
            if visible(blink_y, blink_h) {
                row_label(
                    r,
                    gc,
                    size,
                    scale,
                    &sk,
                    geometry.cursor_blink_row,
                    language.pick("光标闪烁", "Cursor blinking"),
                    "",
                    sk.ink,
                );
            }
            let (or_x, or_y, _, or_h) = geometry.opacity_row;
            let (lr_x, lr_y, _, lr_h) = geometry.language_row;
            if visible(group_y(lr_y), title_h) {
                section_title(
                    r,
                    gc,
                    size,
                    scale,
                    &sk,
                    content_x + s(24.0),
                    group_y(lr_y),
                    language.pick("界面", "Interface"),
                );
            }
            if visible(lr_y, lr_h) {
                r.draw_chrome_text(
                    size,
                    lr_x + s(16.0),
                    row_text_y(lr_y, lr_h),
                    sk.ink,
                    language.pick("语言", "Language"),
                    gc,
                );
                combobox_value(
                    r,
                    gc,
                    geometry.language_row,
                    language_label(view.language_preference, language),
                    sk.accent,
                );
            }
            if visible(geometry.density_row.1, geometry.density_row.3) {
                let (dr_x, dr_y, _, dr_h) = geometry.density_row;
                r.draw_chrome_text(
                    size,
                    dr_x + s(16.0),
                    row_text_y(dr_y, dr_h),
                    sk.ink,
                    language.pick("界面外观", "Appearance"),
                    gc,
                );
                combobox_value(
                    r,
                    gc,
                    geometry.density_row,
                    density_label(view.density, language),
                    sk.accent,
                );
            }
            if visible(or_y, or_h) {
                let stacked = or_h >= s(56.0);
                let text_y = if stacked { or_y + s(9.0) } else { or_y + (or_h - cell_h) / 2.0 };
                r.draw_chrome_text(
                    size,
                    or_x + s(16.0),
                    text_y,
                    sk.ink,
                    language.pick("终端正文不透明度", "Terminal content opacity"),
                    gc,
                );
                let opacity_v = format!("{:.0}%", view.opacity * 100.0);
                let opacity_cols: usize = opacity_v.chars().map(|c| c.width().unwrap_or(0)).sum();
                r.draw_chrome_text(
                    size,
                    if stacked {
                        or_x + geometry.opacity_row.2 - s(16.0) - opacity_cols as f32 * cell_w
                    } else {
                        geometry.opacity_slider.0 - s(10.0) - opacity_cols as f32 * cell_w
                    },
                    text_y,
                    sk.accent,
                    &opacity_v,
                    gc,
                );
            }
            let (br_x, br_y, _, br_h) = geometry.blur;
            if visible(br_y, br_h) {
                // 文案说的是效果不是实现：用户认的是"背景糊不糊"，不是
                // Mica 这个 Windows 专有名词——而且这个开关在 macOS 上走的
                // 是另一套实现。
                r.draw_chrome_text(
                    size,
                    br_x + s(16.0),
                    row_text_y(br_y, br_h),
                    sk.ink,
                    language.pick("背景模糊", "Blur behind window"),
                    gc,
                );
            }
            let (fa_x, fa_y, _, fa_h) = geometry.font_size_row;
            if visible(group_y(fa_y), title_h) {
                section_title(
                    r,
                    gc,
                    size,
                    scale,
                    &sk,
                    fa_x,
                    group_y(fa_y),
                    language.pick("终端外观", "Terminal appearance"),
                );
            }
            if visible(fa_y, fa_h) {
                row_label(
                    r,
                    gc,
                    size,
                    scale,
                    &sk,
                    geometry.font_size_row,
                    language.pick("终端字号（Ctrl+滚轮缩放）", "Font size (Ctrl+wheel zooms)"),
                    "",
                    sk.ink,
                );
                let (value_box, _, _) = widgets::spinner_rects(geometry.font_size_row, scale);
                let value = format!("{:.0}", view.font_size_px);
                let cols: usize = value.chars().map(|c| c.width().unwrap_or(0)).sum();
                r.draw_chrome_text(
                    size,
                    value_box.0 + (value_box.2 - cols as f32 * cell_w) / 2.0,
                    value_box.1 + (value_box.3 - cell_h) / 2.0,
                    sk.ink,
                    &value,
                    gc,
                );
            }
            if visible(geometry.cell_width_mode.1, geometry.cell_width_mode.3) {
                row_label(
                    r,
                    gc,
                    size,
                    scale,
                    &sk,
                    geometry.cell_width_mode,
                    language.pick("字体间距", "Font spacing"),
                    "",
                    sk.ink,
                );
                combobox_value(
                    r,
                    gc,
                    geometry.cell_width_mode,
                    cell_width_mode_label(view.cell_width_mode, language),
                    sk.accent,
                );
            }
            if visible(geometry.fetch.1, geometry.fetch.3) {
                row_label(
                    r,
                    gc,
                    size,
                    scale,
                    &sk,
                    geometry.fetch,
                    language.pick("启动欢迎信息", "Startup welcome"),
                    "",
                    sk.ink,
                );
            }
            if visible(geometry.powerline.1, geometry.powerline.3) {
                row_label(
                    r,
                    gc,
                    size,
                    scale,
                    &sk,
                    geometry.powerline,
                    language.pick("Powerline 提示符", "Powerline prompt"),
                    "",
                    sk.ink,
                );
            }
}
