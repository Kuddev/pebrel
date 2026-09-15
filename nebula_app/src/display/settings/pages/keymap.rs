// Settings UI: Keymap page (search, editable rows, read-only extras, clash hints).

use crate::display::ui::tokens;
use crate::display::color::Rgb;
use crate::display::ui::surface;
use crate::display::ui::text_field;
use crate::display::ui::theme::Skin;
use crate::display::ui::widgets;
use crate::display::{SettingsHit, SizeInfo, UiLanguage};
use crate::display::settings::geometry::SettingsGeometry;
use crate::display::settings::view::SettingsView;
use crate::display::settings::settings_toggle_slot;
use crate::renderer::ui::{Rgba, UiQuad};
use crate::renderer::{GlyphCache, Renderer};
use unicode_width::UnicodeWidthChar;

pub(crate) fn push_keymap_quads(
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
            let cell_w = size.cell_width();
            // 搜索框（原型 .search：input 底、聚焦 accent 边——push_input
            // 配方已含这两态）。捕获进行时搜索不聚焦，caret 不闪。
            {
                let (sx, sy, sw, sh) = geometry.keymap_search;
                let focused = view.keymap_search_focus && view.keymap_capture.is_none();
                let mut input = Vec::new();
                surface::push_input(
                    &mut input,
                    (sx, sy, sw, sh),
                    scale,
                    &sk,
                    view.density,
                    focused,
                );
                text_field::push_cursor(
                    &mut input,
                    sy,
                    sh,
                    sx + s(12.0),
                    &view.keymap_query,
                    &view.keymap_query_cursor,
                    cell_w,
                    scale,
                    &sk,
                );
                for quad in input {
                    clip(quads, quad);
                }
            }
            // 冲突提示条：warn 变体（有待办动作才配警示色，纪律见原型 451）。
            if geometry.keymap_pane.clash {
                let (nx, ny, _nw, nh) = geometry.keymap_note;
                // 警告是语义状态，不再退回被废弃的整块卡片：一条高对比
                // amber beam 保留提示作用，同时让内容继续和页面背景融为一体。
                let beam_w = s(3.0);
                let mark_d = s(16.0);
                clip(
                    quads,
                    UiQuad::solid(
                        nx,
                        ny + s(5.0),
                        beam_w,
                        (nh - s(10.0)).max(0.0),
                        beam_w * 0.5,
                        Rgba::new(sk.warn.r, sk.warn.g, sk.warn.b, 225),
                    ),
                );
                clip(
                    quads,
                    UiQuad::solid(
                        nx + s(8.0),
                        ny + s(10.0),
                        mark_d,
                        mark_d,
                        mark_d * 0.5,
                        Rgba::new(sk.warn.r, sk.warn.g, sk.warn.b, 38),
                    ),
                );
            }
            let (row_x, _, row_w, row_h) = geometry.keymap_row0;
            // 每个动作分组各自一圈 hairline；中心回填页面本色而不是 card 色，
            // 所以只得到独立线框，没有用户不需要的分组背景块。
            let outline_group = |quads: &mut Vec<UiQuad>, first_y: f32, rows: usize| {
                if rows == 0 {
                    return;
                }
                let rect = (row_x, first_y, row_w, row_h * rows as f32);
                let corner = s(tokens::radius::OVERLAY);
                let mut outline = Vec::new();
                surface::push_stroke(&mut outline, rect, corner, scale, sk.hairline);
                outline.push(UiQuad::solid(rect.0, rect.1, rect.2, rect.3, corner, sk.panel));
                for row in 1..rows {
                    outline.push(UiQuad::solid(
                        rect.0,
                        rect.1 + row as f32 * row_h,
                        rect.2,
                        s(1.0).max(1.0),
                        0.0,
                        sk.hairline,
                    ));
                }
                for quad in outline {
                    clip(quads, quad);
                }
            };
            let mut first_slot = 0usize;
            for rows in geometry.keymap_pane.visible {
                let rows = rows as usize;
                if rows > 0 && first_slot < geometry.keymap_slot_ys.len() {
                    outline_group(quads, geometry.keymap_slot_ys[first_slot], rows);
                    first_slot += rows;
                }
            }
            if !view.keymap_readonly_visible.is_empty() {
                outline_group(
                    quads,
                    geometry.keymap_readonly_row0.1,
                    view.keymap_readonly_visible.len(),
                );
            }
            for (slot, flat) in view.keymap_visible.iter().copied().enumerate() {
                if slot >= geometry.keymap_slot_ys.len() {
                    break;
                }
                let rect = (row_x, geometry.keymap_slot_ys[slot], row_w, row_h);
                let hovered = view.hover == SettingsHit::KeymapRow(slot);
                row_hover(quads, rect, hovered);
                // Keycap 底座：捕获中的行换 accent 描边 + 软填充提示「正在
                // 等待按键」；未绑定行不画底座；冲突行 danger 底（.kbd.clash）。
                let capturing = view.keymap_capture == Some(flat);
                let (label, _, bound) = super::super::keymap_row_value(view, flat);
                let cap = super::super::keymap_keycap_rect(rect, &label, cell_w, scale);
                let (cx, cy, cw, ch) = cap;
                if capturing {
                    clip(
                        quads,
                        UiQuad::solid(
                            cx - s(1.0),
                            cy - s(1.0),
                            cw + s(2.0),
                            ch + s(2.0),
                            s(7.0),
                            Rgba::new(sk.accent.r, sk.accent.g, sk.accent.b, 255),
                        ),
                    );
                    clip(quads, UiQuad::solid(cx, cy, cw, ch, s(6.0), sk.panel));
                    clip(quads, UiQuad::solid(cx, cy, cw, ch, s(6.0), sk.accent_soft));
                } else if bound {
                    let combo = crate::display::ui::keycap::layout_combo(
                        &label,
                        rect.0 + rect.2 - s(16.0),
                        rect.1 + rect.3 / 2.0,
                        cell_w,
                        scale,
                    );
                    let danger = view.keymap_clash_rows.get(flat).copied().unwrap_or(false);
                    let (_, chip_y, _, chip_h) = combo.bounds;
                    let mut chip_quads = Vec::new();
                    for &(chip_x, chip_w, _) in &combo.chips {
                        crate::display::ui::keycap::push_chip_toned(
                            &mut chip_quads,
                            &sk,
                            chip_x,
                            chip_y,
                            chip_w,
                            chip_h,
                            scale,
                            hovered,
                            danger,
                        );
                    }
                    for quad in chip_quads {
                        clip(quads, quad);
                    }
                }
            }
            // 只读行同样给轻量 hover（无位移的色变）：一列可交互行里没有
            // 反馈的行读作「死区」，像是渲染坏了。
            let (rx, ry, rw, rh) = geometry.keymap_readonly_row0;
            for index in 0..view.keymap_readonly_visible.len() {
                let rect = (rx, ry + index as f32 * rh, rw, rh);
                row_hover(quads, rect, view.hover == SettingsHit::KeymapReadonlyRow(index));
            }
    quads.extend(staged.drain(..));
}

pub(crate) fn draw_keymap_text(
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
    _icon_draws: &mut Vec<(String, (f32, f32, f32, f32))>,
    _content_x: f32,
    _px: f32,
    clip_top: f32,
    clip_bot: f32,
    title_h: f32,
) {
    let s = |v: f32| v * scale;
    let visible = |ry: f32, rh: f32| ry >= clip_top && ry + rh <= clip_bot;
    let row_text_y = |ry: f32, rh: f32| {
        if geometry.stacked_rows { ry + s(9.0) } else { ry + (rh - cell_h) / 2.0 }
    };
    let group_y = |row_y: f32| row_y - s(42.0);
            // 搜索框文字：查询串或占位；聚焦态的 caret 在 quad pass。
            {
                let (sx, sy, _, sh) = geometry.keymap_search;
                if visible(sy, sh) {
                    let showing = !view.keymap_query.is_empty();
                    let text = if showing {
                        view.keymap_query.clone()
                    } else {
                        language.pick("搜索动作或按键…", "Search actions or keys…").to_owned()
                    };
                    let ink = if showing { sk.ink } else { sk.ink_faint };
                    r.draw_chrome_text(
                        size,
                        sx + s(12.0),
                        sy + (sh - cell_h) / 2.0,
                        ink,
                        &text,
                        gc,
                    );
                }
            }
            // 冲突提示句必须写清哪个绑定不生效，不能让配置静默失效。
            if let Some(note) = &view.keymap_clash_note {
                let (nx, ny, nw, nh) = geometry.keymap_note;
                if geometry.keymap_pane.clash && visible(ny, nh) {
                    let icon_y = ny + (nh - cell_h) / 2.0;
                    let warn_ink = Rgb::new(sk.warn.r, sk.warn.g, sk.warn.b);
                    r.draw_chrome_text(size, nx + s(9.0), icon_y, warn_ink, "!", gc);
                    let max_cols =
                        (((nw - s(38.0)).max(cell_w)) / cell_w).floor().max(1.0) as usize;
                    let lines = super::super::render::warning_lines(note, max_cols);
                    let text_x = nx + s(30.0);
                    r.draw_chrome_text(size, text_x, ny + s(6.0), warn_ink, &lines[0], gc);
                    if !lines[1].is_empty() {
                        r.draw_chrome_text(
                            size,
                            text_x,
                            ny + s(6.0) + cell_h,
                            sk.ink_dim,
                            &lines[1],
                            gc,
                        );
                    }
                }
            }
            // 分组标题（无框分组：标题 + 间距承担层级）；下标 5 = 固定组。
            for (group, title_y) in geometry.keymap_title_ys.iter().enumerate() {
                if !title_y.is_finite() || !visible(*title_y, title_h) {
                    continue;
                }
                let (zh, en) = match super::super::keymap::GROUPS.get(group) {
                    Some((zh, en, _)) => (*zh, *en),
                    None => ("固定快捷键", "Fixed shortcuts"),
                };
                super::super::render::keymap_group_title(
                    r,
                    gc,
                    size,
                    geometry.keymap_row0.0 + s(4.0),
                    *title_y,
                    language.pick(zh, en),
                    sk.ink_dim,
                );
            }
            let (kx, _, kw, kh) = geometry.keymap_row0;
            // 行矩形按文字行剔除：quad 走 scissor 能画半行，文字只要居中
            // 的字盒仍完整落在视口内就照画——否则底部半行只剩空 keycap。
            let line_visible = |ry: f32, rh: f32| {
                let ty = ry + (rh - cell_h) / 2.0;
                ty >= clip_top && ty + cell_h <= clip_bot
            };
            if view.keymap_visible.is_empty() {
                let (_, ey, ..) = geometry.keymap_row0;
                if visible(ey, kh) {
                    r.draw_chrome_text(
                        size,
                        kx + s(4.0),
                        ey + s(6.0),
                        sk.ink_faint,
                        language.pick("没有匹配的动作或按键。", "No actions or keys match."),
                        gc,
                    );
                }
            }
            for (slot, flat) in view.keymap_visible.iter().copied().enumerate() {
                if slot >= geometry.keymap_slot_ys.len() {
                    break;
                }
                let rect = (kx, geometry.keymap_slot_ys[slot], kw, kh);
                if !line_visible(rect.1, rect.3) {
                    continue;
                }
                let i = flat;
                let ty = rect.1 + (kh - cell_h) / 2.0;
                let (zh_label, en_label) = if i == super::super::keymap::QUICK_TERMINAL_ROW {
                    if view.quick_hotkey_error.is_some() {
                        ("快速终端（注册失败）", "Quick terminal (failed)")
                    } else {
                        ("快速终端", "Quick terminal")
                    }
                } else {
                    let (_, zh, en) = super::super::keymap::EDITABLE_ACTIONS[i - 1];
                    (zh, en)
                };
                r.draw_chrome_text(
                    size,
                    rect.0 + s(16.0),
                    ty,
                    if i == super::super::keymap::QUICK_TERMINAL_ROW && view.quick_hotkey_error.is_some() {
                        if sk.is_light { Rgb::new(207, 34, 46) } else { Rgb::new(248, 81, 73) }
                    } else {
                        sk.ink
                    },
                    language.pick(zh_label, en_label),
                    gc,
                );
                let hovered = view.hover == SettingsHit::KeymapRow(slot);
                let capturing = view.keymap_capture == Some(i);
                let (value, customized, bound) = super::super::keymap_row_value(view, i);
                let clash = view.keymap_clash_rows.get(i).copied().unwrap_or(false);
                // 墨色分级（2026-08-09 对齐原型 .kbd）：冲突 danger、捕获/
                // 自定义 accent、hover 提一档、默认键 ink_dim（回滚: sk.ink）、
                // 未绑定 ink_faint（回滚: sk.ink_dim）。
                let ink = if capturing {
                    sk.accent
                } else if clash {
                    Rgb::new(sk.danger.r, sk.danger.g, sk.danger.b)
                } else if hovered && bound {
                    sk.ink_strong
                } else if customized {
                    sk.accent
                } else if bound {
                    sk.ink_dim
                } else {
                    sk.ink_faint
                };
                if capturing || !bound {
                    // 捕获提示 / 未绑定占位仍是整段文本（不是键位展示）。
                    let (cap_x, ..) = super::super::keymap_keycap_rect(rect, &value, cell_w, scale);
                    r.draw_chrome_text(size, cap_x + s(12.0), ty, ink, &value, gc);
                } else {
                    // 键帽规范：一颗 chip 承载整串键位（Windows 心智）。
                    let combo = crate::display::ui::keycap::layout_combo(
                        &value,
                        rect.0 + rect.2 - s(16.0),
                        rect.1 + rect.3 / 2.0,
                        cell_w,
                        scale,
                    );
                    for (chip_x, chip_w, key) in &combo.chips {
                        let key_cols: usize = key.chars().map(|c| c.width().unwrap_or(0)).sum();
                        r.draw_chrome_text(
                            size,
                            chip_x + (chip_w - key_cols as f32 * cell_w) / 2.0,
                            ty,
                            ink,
                            key,
                            gc,
                        );
                    }
                    // hover 才浮现「改键」提示（原型 .rebind 语义）：整行
                    // 本就可点，这里只补可见性，不加新命中。
                    if hovered && !capturing {
                        let rebind = language.pick("改键", "Rebind");
                        let rebind_cols: usize =
                            rebind.chars().map(|c| c.width().unwrap_or(1)).sum();
                        r.draw_chrome_text(
                            size,
                            combo.bounds.0 - s(12.0) - rebind_cols as f32 * cell_w,
                            ty,
                            sk.accent,
                            rebind,
                            gc,
                        );
                    }
                }
            }
            let (rx, ry, rw, rh) = geometry.keymap_readonly_row0;
            for (row, flat) in view.keymap_readonly_visible.iter().copied().enumerate() {
                let Some((zh_label, en_label, combo)) = super::super::keymap::READONLY_ROWS.get(flat) else {
                    continue;
                };
                let rect = (rx, ry + row as f32 * rh, rw, rh);
                if line_visible(rect.1, rect.3) {
                    super::super::render::row_label(
                        r,
                        gc,
                        size,
                        scale,
                        &sk,
                        rect,
                        view.language.pick(zh_label, en_label),
                        combo,
                        sk.ink_dim,
                    );
                }
            }
            // 页尾提示：改键入口与恢复默认的操作说明（原页首长标题的归宿）。
            if visible(geometry.keymap_hint_y, cell_h) {
                r.draw_chrome_text(
                    size,
                    kx + s(4.0),
                    geometry.keymap_hint_y,
                    sk.ink_faint,
                    language.pick(
                        "点击行改键 · 捕获时按 Backspace 恢复默认绑定",
                        "Click a row to rebind · Backspace during capture restores the default",
                    ),
                    gc,
                );
            }
}
