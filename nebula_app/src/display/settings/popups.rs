use crate::display::background_color_model::hsv_to_rgb;
use crate::display::background_color_model::BACKGROUND_SWATCHES;
use crate::display::caret_blink_on;
use crate::display::ui::{icons, surface, text_field, tokens, widgets};
use crate::display::{contains_rect, NebulaSettingsSection, SizeInfo, truncate_tab_label};
use crate::renderer::ui::{Rgba, UiQuad};
use crate::renderer::{GlyphCache, Renderer};
use unicode_width::UnicodeWidthChar;

use super::geometry::{
    background_color_popup, dropdown_anchor, fit_provider_rows, font_popup_row_count,
    font_popup_slot, font_popup_window, popup_visible_index, settings_geometry, SettingsGeometry,
};
use super::view::{
    dropdown_hover_index, dropdown_selected_index, keymap_pane_state_view, proxy_pane_state,
    SettingsView, sync_input_display,
};
use super::{
    accept_label, background_image_alignment_label, background_image_fit_label,
    backup_protocol_label, cell_width_mode_label, completion_style_label, cursor_shape_label,
    density_label, language_label, manual_proxy_protocol_label, new_tab_position_label,
    settings_skin, ssh_proxy_mode_label, tab_reveal_label, SettingsDropdown, SettingsHit,
    ACCEPT_OPTIONS, BACKGROUND_ALIGNMENT_OPTIONS, BACKGROUND_FIT_OPTIONS, BACKUP_PROTOCOL_OPTIONS,
    CELL_WIDTH_MODE_OPTIONS, COMPLETION_STYLE_OPTIONS, CURSOR_SHAPE_OPTIONS, DENSITY_OPTIONS,
    LANGUAGE_OPTIONS, MANUAL_PROXY_PROTOCOL_OPTIONS, NEW_TAB_POSITION_OPTIONS,
    SSH_PROXY_MODE_OPTIONS, TAB_REVEAL_OPTIONS,
};

pub(crate) fn push_popup_quads(
    view: &SettingsView,
    quads: &mut Vec<UiQuad>,
    size: &SizeInfo,
    scale: f32,
) {
    let Some(dropdown) = view.dropdown else { return };
    let s = |v: f32| v * scale;
    let sk = settings_skin(view.theme);
    let mut geometry = settings_geometry(
        size,
        scale,
        view.area,
        view.scroll,
        view.hidden_hosts.len(),
        view.ssh_hosts.len(),
        view.density,
        proxy_pane_state(view),
        keymap_pane_state_view(view),
    );
    fit_provider_rows(&mut geometry, view.providers.len());
    // 背景色专用浮层：色板网格 + hex 输入框（几何与 hit 同源）。
    if dropdown == SettingsDropdown::BackgroundColor {
        if view.section != NebulaSettingsSection::Appearance {
            return;
        }
        let popup = background_color_popup(&geometry, scale);
        let (px2, py2, pw2, ph2) = popup.rect;
        // 与通用 combobox 浮层同一套皮肤：柔和投影 + hairline + 不透明面板。
        quads.push(UiQuad::glow(
            px2 - s(14.0),
            py2 - s(10.0),
            pw2 + s(28.0),
            ph2 + s(26.0),
            Rgba::new(0, 0, 0, 70),
        ));
        quads.push(UiQuad::solid(
            px2 - s(1.0),
            py2 - s(1.0),
            pw2 + s(2.0),
            ph2 + s(2.0),
            s(11.0),
            sk.hairline,
        ));
        let mut plate = sk.panel;
        plate.a = 255;
        quads.push(UiQuad::solid(px2, py2, pw2, ph2, s(10.0), plate));
        quads.push(UiQuad::solid(px2, py2, pw2, ph2, s(10.0), sk.surface));

        // ---- 真调色盘：SV 取色面 + 色相条 ----
        // 连续渐变用小色块阵列近似：UiQuad 只有纯色，24×16 的格阵在
        // 128px 高的面上每格 ~8px，肉眼已无明显色带；总量 ~400 quad，
        // 相对设置页整体的 quad 预算可忽略。
        let (h0, s0, v0) = view.bg_picker_hsv;
        let (svx, svy, svw, svh) = popup.sv;
        surface::push_stroke(quads, popup.sv, tokens::radius::CHIP * scale, scale, sk.hairline);
        const SV_COLS: usize = 24;
        const SV_ROWS: usize = 16;
        let cell_w = svw / SV_COLS as f32;
        let cell_h = svh / SV_ROWS as f32;
        for row in 0..SV_ROWS {
            for col in 0..SV_COLS {
                // 格中心取样：边缘格也能到达 s/v 的 0 和 1 近旁。
                let sat = (col as f32 + 0.5) / SV_COLS as f32;
                let val = 1.0 - (row as f32 + 0.5) / SV_ROWS as f32;
                let c = hsv_to_rgb(h0, sat, val);
                quads.push(UiQuad::solid(
                    svx + col as f32 * cell_w,
                    svy + row as f32 * cell_h,
                    cell_w + 0.5,
                    cell_h + 0.5,
                    0.0,
                    Rgba::new(c.r, c.g, c.b, 255),
                ));
            }
        }
        // 当前取点：白环 + 黑环双圈，在亮暗底上都可见。
        let dot_x = svx + s0.clamp(0.0, 1.0) * svw;
        let dot_y = svy + (1.0 - v0.clamp(0.0, 1.0)) * svh;
        let dot_r = s(6.0);
        quads.push(UiQuad::solid(
            dot_x - dot_r,
            dot_y - dot_r,
            dot_r * 2.0,
            dot_r * 2.0,
            dot_r,
            Rgba::new(255, 255, 255, 255),
        ));
        quads.push(UiQuad::solid(
            dot_x - dot_r + s(1.5),
            dot_y - dot_r + s(1.5),
            (dot_r - s(1.5)) * 2.0,
            (dot_r - s(1.5)) * 2.0,
            dot_r - s(1.5),
            Rgba::new(0, 0, 0, 200),
        ));
        let picked = hsv_to_rgb(h0, s0, v0);
        quads.push(UiQuad::solid(
            dot_x - dot_r + s(3.0),
            dot_y - dot_r + s(3.0),
            (dot_r - s(3.0)) * 2.0,
            (dot_r - s(3.0)) * 2.0,
            dot_r - s(3.0),
            Rgba::new(picked.r, picked.g, picked.b, 255),
        ));

        // 色相条：36 段近似 0–360°，游标为竖向双色线。
        let (hux, huy, huw, huh) = popup.hue;
        surface::push_stroke(quads, popup.hue, tokens::radius::CHIP * scale, scale, sk.hairline);
        const HUE_STEPS: usize = 36;
        let hue_w = huw / HUE_STEPS as f32;
        for step in 0..HUE_STEPS {
            let hue = (step as f32 + 0.5) / HUE_STEPS as f32 * 360.0;
            let c = hsv_to_rgb(hue, 1.0, 1.0);
            quads.push(UiQuad::solid(
                hux + step as f32 * hue_w,
                huy,
                hue_w + 0.5,
                huh,
                0.0,
                Rgba::new(c.r, c.g, c.b, 255),
            ));
        }
        // 游标：白底黑芯的胶囊竖线，在任意色相段上都可见。圆角从自身宽度
        // 派生（胶囊），不是阶梯常量。
        let cursor_x = hux + (h0.rem_euclid(360.0) / 360.0) * huw;
        let cursor_outer = s(4.0);
        let cursor_inner = s(2.0);
        quads.push(UiQuad::solid(
            cursor_x - cursor_outer * 0.5,
            huy - cursor_inner,
            cursor_outer,
            huh + cursor_inner * 2.0,
            cursor_outer * 0.5,
            Rgba::new(255, 255, 255, 255),
        ));
        quads.push(UiQuad::solid(
            cursor_x - cursor_inner * 0.5,
            huy - cursor_inner * 0.5,
            cursor_inner,
            huh + cursor_inner,
            cursor_inner * 0.5,
            Rgba::new(0, 0, 0, 180),
        ));

        let selected = dropdown_selected_index(view, dropdown);
        for (index, rect) in popup.swatch.iter().enumerate() {
            let (sx, sy, sw2, sh2) = *rect;
            let hovered = view.hover == SettingsHit::BackgroundSwatch(index);
            if selected == Some(index) || hovered {
                let ring = if selected == Some(index) { sk.accent } else { sk.ink_dim };
                quads.push(UiQuad::solid(
                    sx - s(2.0),
                    sy - s(2.0),
                    sw2 + s(4.0),
                    sh2 + s(4.0),
                    s(8.0),
                    Rgba::new(ring.r, ring.g, ring.b, 255),
                ));
            }
            // 每格带 1px hairline 描边：亮色格在浅色面板上也有边界。
            quads.push(UiQuad::solid(
                sx - s(1.0),
                sy - s(1.0),
                sw2 + s(2.0),
                sh2 + s(2.0),
                s(7.0),
                sk.hairline,
            ));
            let color = BACKGROUND_SWATCHES[index];
            quads.push(UiQuad::solid(
                sx,
                sy,
                sw2,
                sh2,
                s(6.0),
                Rgba::new(color.r, color.g, color.b, 255),
            ));
        }

        // hex 输入框：聚焦态用主题色描边；caret 与 UI 光标共用 500ms 相位。
        let (hx, hy, hw, hh) = popup.hex;
        let focused = view.bg_hex_active;
        let border = if focused { sk.accent } else { sk.ink_dim };
        let border_alpha = if focused { 255 } else { 120 };
        quads.push(UiQuad::solid(
            hx - s(1.0),
            hy - s(1.0),
            hw + s(2.0),
            hh + s(2.0),
            s(8.0),
            Rgba::new(border.r, border.g, border.b, border_alpha),
        ));
        quads.push(UiQuad::solid(hx, hy, hw, hh, s(7.0), sk.surface));
        if focused && crate::display::caret_blink_on() {
            let cell_w = size.cell_width();
            let caret_x = hx + s(12.0) + view.bg_hex_input.chars().count() as f32 * cell_w;
            let caret_h = hh - s(12.0);
            quads.push(UiQuad::solid(
                caret_x.min(hx + hw - s(6.0)),
                hy + (hh - caret_h) / 2.0,
                (1.5 * scale).max(1.0),
                caret_h,
                0.0,
                Rgba::new(sk.accent.r, sk.accent.g, sk.accent.b, 255),
            ));
        }
        return;
    }
    let (_, py, _, ph) = geometry.popup;
    let Some((anchor, total)) = dropdown_anchor(
        &geometry,
        view.section,
        dropdown,
        view.shells.len(),
        view.fonts.len() + 1,
        scale,
    ) else {
        return;
    };
    let (offset, count) = if dropdown == SettingsDropdown::Font {
        font_popup_window(total, view.font_popup_scroll)
    } else {
        (0, total)
    };
    let popup =
        widgets::combobox_popup_rect(anchor, count, scale, geometry.content_top, py + ph - s(6.0));
    let selected =
        popup_visible_index(dropdown, dropdown_selected_index(view, dropdown), offset, count);
    let hover =
        popup_visible_index(dropdown, dropdown_hover_index(view.hover, dropdown), offset, count);
    widgets::push_combobox_popup(quads, popup, count, selected, hover, scale, &sk, view.density);
    // 字体弹层第 0 行是一个正经输入框：下沉底 + 光标/选区。它不是选项，所以
    // 不吃 hover 高亮，走 `push_input` 而不是 popup 行的配方。
    if matches!(dropdown, SettingsDropdown::Font) {
        let field = widgets::popup_row_rect(popup, 0, scale);
        surface::push_input(quads, field, scale, &sk, view.density, true);
        text_field::push_cursor(
            quads,
            field.1,
            field.3,
            field.0 + s(12.0),
            &view.font_query,
            &view.font_query_cursor,
            size.cell_width(),
            scale,
            &sk,
        );
    }
    if let Some(index) = selected {
        let (rx, ry, rw, rh) = widgets::popup_row_rect(popup, index, scale);
        icons::push_check(
            quads,
            rx + rw - s(16.0),
            ry + rh * 0.5,
            scale,
            Rgba::new(sk.accent.r, sk.accent.g, sk.accent.b, 255),
        );
    }
    if dropdown == SettingsDropdown::Font {
        let total_h = total as f32 * widgets::POPUP_ROW_H * scale;
        let viewport_h = count as f32 * widgets::POPUP_ROW_H * scale;
        if let Some(scrollbar) = widgets::overlay_scrollbar(
            popup,
            viewport_h,
            total_h,
            offset as f32 * widgets::POPUP_ROW_H * scale,
            scale,
        ) {
            widgets::push_overlay_scrollbar(
                quads,
                scrollbar,
                scale,
                &sk,
                view.font_popup_dragging,
                view.font_popup_dragging,
            );
        }
    }
}

/// Option labels for the floating dropdown; returns shell brand-icon draw
/// requests like [`draw_text`]. Must run AFTER `push_popup_quads`'s quads are
/// painted so the labels sit on top of the popup plate.
pub(crate) fn draw_popup_text(
    view: &SettingsView,
    r: &mut Renderer,
    gc: &mut GlyphCache,
    size: &SizeInfo,
    scale: f32,
) -> Vec<(String, (f32, f32, f32, f32))> {
    let mut icon_draws = Vec::new();
    let Some(dropdown) = view.dropdown else { return icon_draws };
    let s = |v: f32| v * scale;
    let sk = settings_skin(view.theme);
    let language = view.language;
    let cell_w = size.cell_width();
    let cell_h = size.cell_height();
    let geometry = settings_geometry(
        size,
        scale,
        view.area,
        view.scroll,
        view.hidden_hosts.len(),
        view.ssh_hosts.len(),
        view.density,
        proxy_pane_state(view),
        keymap_pane_state_view(view),
    );
    // 背景色浮层：hex 草稿（或占位提示）画进输入框，色板格无文字。
    if dropdown == SettingsDropdown::BackgroundColor {
        if view.section != NebulaSettingsSection::Appearance {
            return icon_draws;
        }
        let popup = background_color_popup(&geometry, scale);
        let (hx, hy, hw, hh) = popup.hex;
        let ty = hy + (hh - cell_h) / 2.0;
        if view.bg_hex_input.is_empty() {
            r.draw_chrome_text(size, hx + s(12.0), ty, sk.ink_dim, "#RRGGBB", gc);
        } else {
            r.draw_chrome_text(size, hx + s(12.0), ty, sk.ink, &view.bg_hex_input, gc);
        }
        // 输入框右侧给一个动作提示（回车应用）。
        let hint = language.pick("回车应用", "Enter applies");
        let hint_cols: usize = hint.chars().map(|c| c.width().unwrap_or(0)).sum();
        let hint_x = hx + hw - s(12.0) - hint_cols as f32 * cell_w;
        if hint_x > hx + s(12.0) + 9.0 * cell_w {
            r.draw_chrome_text(size, hint_x, ty, sk.ink_dim, hint, gc);
        }
        return icon_draws;
    }
    let (_, py, _, ph) = geometry.popup;
    let Some((anchor, total)) = dropdown_anchor(
        &geometry,
        view.section,
        dropdown,
        view.shells.len(),
        view.fonts.len() + 1,
        scale,
    ) else {
        return icon_draws;
    };
    let (offset, count) = if dropdown == SettingsDropdown::Font {
        font_popup_window(total, view.font_popup_scroll)
    } else {
        (0, total)
    };
    let popup =
        widgets::combobox_popup_rect(anchor, count, scale, geometry.content_top, py + ph - s(6.0));
    let selected =
        popup_visible_index(dropdown, dropdown_selected_index(view, dropdown), offset, count);
    for index in 0..count {
        let absolute_index =
            if dropdown == SettingsDropdown::Font && index > 0 { index + offset } else { index };
        let (rx, ry, rw, rh) = widgets::popup_row_rect(popup, index, scale);
        let ty = ry + (rh - cell_h) / 2.0;
        // Shell rows lead with the brand icon; every other list is text-only.
        let mut text_x = rx + s(12.0);
        let label: String = match dropdown {
            SettingsDropdown::Shell => {
                let Some((id, name, program)) = view.shells.get(absolute_index) else { continue };
                icon_draws
                    .push((id.clone(), (rx + s(8.0), ry + (rh - s(24.0)) / 2.0, s(24.0), s(24.0))));
                text_x = rx + s(40.0);
                if program.is_empty() { name.clone() } else { format!("{name}  ·  {program}") }
            },
            SettingsDropdown::Font => {
                // 第 0 行是搜索框：它的底与光标在 quads pass 里画，这里只
                // 落查询串本身（空着时落提示语）。
                let Some(slot) = font_popup_slot(absolute_index) else {
                    let showing = !view.font_query.is_empty();
                    let text = if showing {
                        view.font_query.clone()
                    } else {
                        language
                            .pick("搜索字体…（直接打字）", "Search fonts… (just type)")
                            .to_owned()
                    };
                    let ink = if showing { sk.ink } else { sk.ink_faint };
                    r.draw_chrome_text(size, text_x, ty, ink, &text, gc);
                    continue;
                };
                match view.fonts.get(slot) {
                    // 候选行用**这个字体自己的字形**画自己的名字：选之前就看见
                    // 选之后的样子（WYSIWYG）。chrome 文本按单元格步进排版，
                    // 所以比例字体在这里的挤压与它进终端网格后完全一致——预览
                    // 不美化，正因如此才有判断价值。
                    //
                    // 元信息（「· 非等宽」）留在界面字体里：那是我们的批注，
                    // 不是字体样本，跟着候选字体变形只会让人误读。
                    Some(family) => {
                        let max_chars = (((rx + rw - s(28.0)) - text_x).max(cell_w) / cell_w)
                            .floor()
                            .max(1.0) as usize;
                        let color = if selected == Some(index) { sk.accent } else { sk.ink };
                        let name = truncate_tab_label(family, max_chars);
                        let previewing = gc.begin_preview_face(family);
                        r.draw_chrome_text(size, text_x, ty, color, &name, gc);
                        if previewing {
                            gc.end_preview_face();
                        }
                        // 非等宽批注接在名字之后，按已画列数让位。
                        if view.font_proportional.contains(&family.to_lowercase()) {
                            let cols: usize =
                                name.chars().map(|c| c.width().unwrap_or(0)).sum::<usize>() + 3;
                            let note_x = text_x + cols as f32 * cell_w;
                            let note = language.pick("· 非等宽", "· not monospaced");
                            let note_cols: usize =
                                note.chars().map(|c| c.width().unwrap_or(0)).sum();
                            if note_x + note_cols as f32 * cell_w < rx + rw - s(28.0) {
                                r.draw_chrome_text(size, note_x, ty, sk.ink_dim, note, gc);
                            }
                        }
                        continue;
                    },
                    // 倒数第二行是过滤切换，最后一行是导入。
                    None if slot == view.fonts.len() => {
                        if view.font_show_all {
                            language.pick("◉  显示全部字体", "(*) Showing all fonts").to_owned()
                        } else {
                            language.pick("○  仅等宽字体", "( ) Monospaced only").to_owned()
                        }
                    },
                    None => language.pick("＋  导入字体…", "+  Import font...").to_owned(),
                }
            },
            SettingsDropdown::BackgroundFit => {
                background_image_fit_label(BACKGROUND_FIT_OPTIONS[index], language).to_owned()
            },
            SettingsDropdown::BackgroundAlignment => {
                background_image_alignment_label(BACKGROUND_ALIGNMENT_OPTIONS[index], language)
                    .to_owned()
            },
            SettingsDropdown::Language => {
                language_label(LANGUAGE_OPTIONS[index], language).to_owned()
            },
            SettingsDropdown::Accept => accept_label(ACCEPT_OPTIONS[index], language).to_owned(),
            SettingsDropdown::CompletionStyle => {
                completion_style_label(COMPLETION_STYLE_OPTIONS[index], language).to_owned()
            },
            SettingsDropdown::BackupProtocol => {
                backup_protocol_label(BACKUP_PROTOCOL_OPTIONS[index], language).to_owned()
            },
            SettingsDropdown::TabReveal => {
                tab_reveal_label(TAB_REVEAL_OPTIONS[index], language).to_owned()
            },
            SettingsDropdown::Density => density_label(DENSITY_OPTIONS[index], language).to_owned(),
            SettingsDropdown::NewTabPosition => {
                new_tab_position_label(NEW_TAB_POSITION_OPTIONS[index], language).to_owned()
            },
            SettingsDropdown::CellWidthMode => {
                cell_width_mode_label(CELL_WIDTH_MODE_OPTIONS[index], language).to_owned()
            },
            SettingsDropdown::CursorShape => {
                cursor_shape_label(CURSOR_SHAPE_OPTIONS[index], language).to_owned()
            },
            SettingsDropdown::SshProxyMode => {
                ssh_proxy_mode_label(SSH_PROXY_MODE_OPTIONS[index], language).to_owned()
            },
            SettingsDropdown::SshProxyProtocol => {
                manual_proxy_protocol_label(MANUAL_PROXY_PROTOCOL_OPTIONS[index], language)
                    .to_owned()
            },
            SettingsDropdown::SshJumpHost => match view.ssh_hosts.get(absolute_index) {
                Some(host) if host.label != host.destination => {
                    format!("{}  ·  {}", host.label, host.destination)
                },
                Some(host) => host.destination.clone(),
                // 空列表的占位行：告诉用户去哪里补数据，点击无动作。
                None => language
                    .pick("没有已保存的主机——先在 SSH 页添加", "No saved hosts — add one in SSH")
                    .to_owned(),
            },
            // 上方特判提前返回；此臂只为 match 完备。
            SettingsDropdown::BackgroundColor => continue,
        };
        let import_row = matches!(dropdown, SettingsDropdown::Font)
            && font_popup_slot(absolute_index).is_some_and(|slot| view.fonts.get(slot).is_none());
        let color = if selected == Some(index) || import_row { sk.accent } else { sk.ink };
        let max_chars =
            (((rx + rw - s(28.0)) - text_x).max(cell_w) / cell_w).floor().max(1.0) as usize;
        let label = truncate_tab_label(&label, max_chars);
        r.draw_chrome_text(size, text_x, ty, color, &label, gc);
    }
    icon_draws
}

