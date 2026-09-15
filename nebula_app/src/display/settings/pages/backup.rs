// Settings UI: Backup page (export, restore, auto backup, remote backup).

use crate::display::color::Rgb;
use crate::display::ui::icons;
use crate::display::ui::surface;
use crate::display::ui::theme::Skin;
use crate::display::ui::widgets;
use crate::display::ui::tokens;
use crate::display::{SettingsHit, SettingsDropdown, SizeInfo, UiLanguage};
use crate::display::settings::geometry::SettingsGeometry;
use crate::display::settings::view::SettingsView;
use crate::display::settings::settings_toggle_slot;
use crate::renderer::ui::{Rgba, UiQuad};
use crate::renderer::{GlyphCache, Renderer};
use unicode_width::UnicodeWidthChar;

pub(crate) fn push_backup_quads(
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
    let combobox = |quads: &mut Vec<UiQuad>,
                    staged: &mut Vec<UiQuad>,
                    row, hot: bool, open: bool| {
        widgets::push_combobox(staged, widgets::combobox_rect(row, scale), scale, &sk, hot, open);
        for quad in staged.drain(..) { clip(quads, quad); }
    };

    let overlay_radius = crate::display::ui::tokens::radius::OVERLAY * scale;
            let control_radius = crate::display::ui::tokens::radius::CONTROL * scale;
            let chip_radius = crate::display::ui::tokens::radius::CHIP * scale;
            // Automatic-backup summary card. Its switch is deliberately shown
            // disabled while the page is gated: the current backend supports
            // explicit encrypted exports, but has no scheduled-retention
            // service yet, so presenting an active control would be dishonest.
            {
                let (ax, ay, aw, ah) = geometry.backup_auto;
                let mut stroke = Vec::new();
                surface::push_stroke(
                    &mut stroke,
                    geometry.backup_auto,
                    overlay_radius,
                    scale,
                    sk.hairline,
                );
                for quad in stroke {
                    clip(quads, quad);
                }
                clip(quads, UiQuad::solid(ax, ay, aw, ah, overlay_radius, sk.panel));
                let icon_rect = (ax + s(16.0), ay + (ah - s(34.0)) * 0.5, s(34.0), s(34.0));
                let (ix, iy, iw, ih) = icon_rect;
                clip(quads, UiQuad::solid(ix, iy, iw, ih, control_radius, sk.surface));
                let mut icon = Vec::new();
                icons::push_settings_nav_icon(
                    &mut icon,
                    icons::SettingsNavIcon::Backup,
                    icon_rect,
                    scale,
                    Rgba::new(sk.icon.r, sk.icon.g, sk.icon.b, 220),
                    icons::blend_over(sk.panel, sk.surface),
                );
                for quad in icon {
                    clip(quads, quad);
                }
                let track = (ax + aw - s(50.0), ay + (ah - s(20.0)) * 0.5, s(34.0), s(20.0));
                clip(
                    quads,
                    UiQuad::solid(track.0, track.1, track.2, track.3, track.3 * 0.5, sk.track_off),
                );
                clip(
                    quads,
                    UiQuad::solid(
                        track.0 + s(2.0),
                        track.1 + s(2.0),
                        s(16.0),
                        s(16.0),
                        s(16.0) * 0.5,
                        sk.knob_off,
                    ),
                );
            }

            // Export / restore actions share the prototype's segmented plate.
            {
                let (sx, sy, sw, sh) = geometry.backup_segment;
                clip(quads, UiQuad::solid(sx, sy, sw, sh, overlay_radius, sk.card));
                let [export, restore] = super::super::backup_segment_rects(geometry.backup_segment, scale);
                for (rect, hit, active) in [
                    (export, SettingsHit::BackupExport, true),
                    (restore, SettingsHit::BackupRestore, false),
                ] {
                    let (bx, by, bw, bh) = rect;
                    let fill = if active {
                        sk.panel
                    } else if view.hover == hit {
                        sk.hover
                    } else {
                        Rgba::new(0, 0, 0, 0)
                    };
                    clip(quads, UiQuad::solid(bx, by, bw, bh, control_radius, fill));
                }
            }

            // One manifest card with quiet group headers. Row geometry is not
            // contiguous in category-index order, so paint by the explicit
            // visual order used by the HTML prototype.
            {
                let (gx, gy, gw, _) = geometry.backup_groups[0];
                let last = geometry.backup_rows[8];
                let gh = last.1 + last.3 - gy;
                clip(quads, UiQuad::solid(gx, gy, gw, gh, overlay_radius, sk.panel));
            }
            for (index, row) in geometry.backup_rows.iter().enumerate() {
                row_hover(quads, *row, view.hover == SettingsHit::BackupSelection(index));
                let on = super::super::backup_item_selected(view.backup_selection, index);
                let cb = (row.0 + s(16.0), row.1 + (row.3 - s(16.0)) * 0.5, s(16.0), s(16.0));
                if on {
                    clip(
                        quads,
                        UiQuad::solid(
                            cb.0,
                            cb.1,
                            cb.2,
                            cb.3,
                            chip_radius,
                            Rgba::new(sk.accent.r, sk.accent.g, sk.accent.b, 255),
                        ),
                    );
                    let mut check = Vec::new();
                    icons::push_check(
                        &mut check,
                        cb.0 + cb.2 * 0.5,
                        cb.1 + cb.3 * 0.5,
                        scale * 0.82,
                        Rgba::new(sk.ink_on_accent.r, sk.ink_on_accent.g, sk.ink_on_accent.b, 255),
                    );
                    for quad in check {
                        clip(quads, quad);
                    }
                } else {
                    let mut stroke = Vec::new();
                    surface::push_stroke(&mut stroke, cb, chip_radius, scale, sk.hairline);
                    for quad in stroke {
                        clip(quads, quad);
                    }
                    clip(quads, UiQuad::solid(cb.0, cb.1, cb.2, cb.3, chip_radius, sk.panel));
                }
            }

            // ---- 远程备份：协议下拉 + 按协议裁剪的输入行 + 动作行 ----
            {
                let field_count = crate::backup_remote::field_count(view.backup_protocol);
                let group_rows = 1 + field_count;
                group_frame(quads, geometry.backup_remote_protocol, group_rows);
                row_hover(
                    quads,
                    geometry.backup_remote_protocol,
                    view.hover == SettingsHit::BackupProtocolCycle,
                );
                combobox(
                    quads,
                    &mut staged,
                    geometry.backup_remote_protocol,
                    view.hover == SettingsHit::BackupProtocolCycle,
                    view.dropdown == Some(SettingsDropdown::BackupProtocol),
                );
                let cell_w = size.cell_width();
                for (index, row) in
                    geometry.backup_remote_fields.iter().take(field_count).enumerate()
                {
                    row_hover(quads, *row, view.hover == SettingsHit::BackupRemoteField(index));
                    let rect = super::super::sync_input_rect(*row, scale);
                    let (ix, iy, iw, ih) = rect;
                    let focused = view.backup_remote_focus == Some(index);
                    let border = if focused {
                        Rgba::new(sk.accent.r, sk.accent.g, sk.accent.b, 255)
                    } else {
                        Rgba::new(sk.ink_dim.r, sk.ink_dim.g, sk.ink_dim.b, 90)
                    };
                    let mut stroke = Vec::new();
                    surface::push_stroke(&mut stroke, rect, control_radius, scale, border);
                    for quad in stroke {
                        clip(quads, quad);
                    }
                    clip(quads, UiQuad::solid(ix, iy, iw, ih, control_radius, sk.surface));
                    if focused && crate::display::caret_blink_on() {
                        let max_cols = (((iw - s(24.0)) / cell_w) as usize).max(1);
                        let (_, placeholder, cols) =
                            super::super::backup_remote_input_display(view, index, max_cols);
                        let cols = if placeholder { 0 } else { cols };
                        let caret_h = ih - s(10.0);
                        clip(
                            quads,
                            UiQuad::solid(
                                (ix + s(12.0) + cols as f32 * cell_w).min(ix + iw - s(6.0)),
                                iy + (ih - caret_h) / 2.0,
                                (1.5 * scale).max(1.0),
                                caret_h,
                                0.0,
                                Rgba::new(sk.accent.r, sk.accent.g, sk.accent.b, 255),
                            ),
                        );
                    }
                }
                // 动作行：两个独立按钮（协议关闭时不画，忙时变灰不吃 hover）。
                if view.backup_protocol != crate::backup_remote::BackupProtocol::Off {
                    let actions = super::super::backup_remote_actions_rect(&geometry, scale, field_count);
                    let [push_rect, pull_rect] = super::super::sync_button_rects(actions, scale);
                    for (rect, hit) in [
                        (push_rect, SettingsHit::BackupRemotePush),
                        (pull_rect, SettingsHit::BackupRemotePull),
                    ] {
                        let (bx, by, bw, bh) = rect;
                        let hot = view.hover == hit && !view.backup_busy;
                        let mut stroke = Vec::new();
                        surface::push_stroke(&mut stroke, rect, control_radius, scale, sk.hairline);
                        for quad in stroke {
                            clip(quads, quad);
                        }
                        clip(
                            quads,
                            UiQuad::solid(
                                bx,
                                by,
                                bw,
                                bh,
                                control_radius,
                                if hot { sk.hover } else { sk.panel },
                            ),
                        );
                    }
                }
}
            }

pub(crate) fn draw_backup_text(
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
    content_x: f32,
    _px: f32,
    clip_top: f32,
    clip_bot: f32,
    title_h: f32,
) {
    let s = |v: f32| v * scale;
    let visible = |ry: f32, rh: f32| ry >= clip_top && ry + rh <= clip_bot;
    let group_y = |row_y: f32| row_y - s(42.0);
    let combobox_value = |r: &mut Renderer,
                         gc: &mut GlyphCache,
                         row: (f32, f32, f32, f32),
                         value: &str,
                         ink: Rgb| {
        let rect = widgets::combobox_rect(row, scale);
        let tx = widgets::combobox_text_x(rect, scale);
        let right = widgets::combobox_text_right(rect, scale);
        let max_chars = ((right - tx).max(cell_w) / cell_w).floor().max(1.0) as usize;
        let value = super::super::truncate_tab_label(value, max_chars);
        r.draw_chrome_text(size, tx, rect.1 + (rect.3 - cell_h) / 2.0, ink, &value, gc);
    };
    let row_text_y = |ry: f32, rh: f32| {
        if geometry.stacked_rows { ry + s(9.0) } else { ry + (rh - cell_h) / 2.0 }
    };
    let (_, content_y, _, _) = geometry.content;
            r.draw_chrome_text(
                size,
                content_x + s(24.0),
                content_y + s(49.0),
                sk.ink_faint,
                language.pick(
                    "导出、恢复与自动备份 · 加密文件可跨设备迁移",
                    "Export, restore, and automatic backups · encrypted and portable",
                ),
                gc,
            );

            let (ax, ay, _, ah) = geometry.backup_auto;
            if visible(ay, ah) {
                r.draw_chrome_text(
                    size,
                    ax + s(64.0),
                    ay + s(11.0),
                    sk.ink_strong,
                    language.pick("自动备份", "Automatic backup"),
                    gc,
                );
                r.draw_chrome_text(
                    size,
                    ax + s(64.0),
                    ay + s(11.0) + cell_h,
                    sk.ink_faint,
                    language.pick(
                        "恢复预览与回滚流程完成后开放，当前请使用手动导出",
                        "Available after restore preview and rollback are complete; use manual export for now",
                    ),
                    gc,
                );
            }

            let [export, restore] = super::super::backup_segment_rects(geometry.backup_segment, scale);
            for ((bx, by, bw, bh), caption, active) in [
                (export, language.pick("导出备份", "Export backup"), true),
                (restore, language.pick("恢复备份", "Restore backup"), false),
            ] {
                if visible(by, bh) {
                    let cols =
                        caption.chars().map(|c| c.width().unwrap_or(1).max(1)).sum::<usize>();
                    r.draw_chrome_text(
                        size,
                        bx + (bw - cols as f32 * cell_w) / 2.0,
                        by + (bh - cell_h) / 2.0,
                        if active { sk.ink_strong } else { sk.ink_dim },
                        caption,
                        gc,
                    );
                }
            }

            let title_y = geometry.backup_groups[0].1 - s(30.0);
            if visible(title_y, title_h) {
                super::super::render::section_title(
                    r,
                    gc,
                    size,
                    scale,
                    &sk,
                    geometry.backup_groups[0].0,
                    title_y,
                    language.pick("导出内容", "Backup contents"),
                );
            }

            let group_labels = [
                language.pick("设置  ·  3 项", "SETTINGS  ·  3 ITEMS"),
                language.pick("SSH  ·  1 项", "SSH  ·  1 ITEM"),
                language.pick("AI  ·  1 项", "AI  ·  1 ITEM"),
                language.pick("数据与历史  ·  4 项", "DATA & HISTORY  ·  4 ITEMS"),
            ];
            for (group, label) in geometry.backup_groups.iter().zip(group_labels) {
                if visible(group.1, group.3) {
                    r.draw_chrome_text(
                        size,
                        group.0 + s(16.0),
                        group.1 + (group.3 - cell_h) / 2.0,
                        sk.ink_faint,
                        label,
                        gc,
                    );
                }
            }

            let items = [
                (
                    ("外观", "Appearance"),
                    ("主题、字号、透明度与背景", "Theme, font size, opacity, and background"),
                    "12 KB",
                ),
                (
                    ("配置文件", "Profiles"),
                    ("Shell 配置与启动参数", "Shell profiles and launch arguments"),
                    "4 KB",
                ),
                (
                    ("SSH 地址簿", "SSH address book"),
                    (
                        "主机、端口、代理与认证方式，不含凭据",
                        "Hosts, ports, proxies, and auth methods; no credentials",
                    ),
                    "2 KB",
                ),
                (
                    ("同步配置", "Sync configuration"),
                    ("WebDAV 地址与同步策略，不含密码", "WebDAV endpoint and policy; no passwords"),
                    "1 KB",
                ),
                (
                    ("AI 助手", "AI assistant"),
                    (
                        "模型、MCP 与 Skills 开关，不含 API Key",
                        "Models, MCP, and Skills switches; no API keys",
                    ),
                    "6 KB",
                ),
                (
                    ("会话布局", "Session layout"),
                    ("标签页、分屏与工作区", "Tabs, splits, and workspaces"),
                    "3 KB",
                ),
                (
                    ("目录历史", "Directory history"),
                    ("最近目录，用于启动器推荐", "Recent folders used by launcher suggestions"),
                    "18 KB",
                ),
                (
                    ("命令历史", "Command history"),
                    ("各会话保存的本地命令记录", "Locally stored command history by session"),
                    "64 KB",
                ),
                (
                    ("导入字体", "Imported fonts"),
                    ("Nebula 管理的私有字体文件", "Private font files managed by Nebula"),
                    "2.4 MB",
                ),
            ];
            for (index, row) in geometry.backup_rows.iter().enumerate() {
                if visible(row.1, row.3) {
                    let (label, description, meta) = items[index];
                    let text_x = row.0 + s(44.0);
                    let meta_cols = meta.chars().count();
                    let meta_x = row.0 + row.2 - s(16.0) - meta_cols as f32 * cell_w;
                    let max_desc_cols =
                        (((meta_x - s(12.0) - text_x) / cell_w).floor() as usize).max(1);
                    let description = super::super::truncate_tab_label(
                        language.pick(description.0, description.1),
                        max_desc_cols,
                    );
                    r.draw_chrome_text(
                        size,
                        text_x,
                        row.1 + s(7.0),
                        if super::super::backup_item_selected(view.backup_selection, index) {
                            sk.ink
                        } else {
                            sk.ink_dim
                        },
                        language.pick(label.0, label.1),
                        gc,
                    );
                    r.draw_chrome_text(
                        size,
                        text_x,
                        row.1 + s(7.0) + cell_h,
                        sk.ink_faint,
                        &description,
                        gc,
                    );
                    r.draw_chrome_text(
                        size,
                        meta_x,
                        row.1 + (row.3 - cell_h) / 2.0,
                        sk.ink_faint,
                        meta,
                        gc,
                    );
                }
            }
            // ---- 远程备份 ----
            let field_count = crate::backup_remote::field_count(view.backup_protocol);
            {
                let (rx, ry, _, rh) = geometry.backup_remote_protocol;
                let remote_title_y = ry - s(30.0);
                if visible(remote_title_y, title_h) {
                    super::super::render::section_title(
                        r,
                        gc,
                        size,
                        scale,
                        &sk,
                        rx,
                        remote_title_y,
                        language.pick("远程备份", "Remote backup"),
                    );
                }
                if visible(ry, rh) {
                    super::super::render::row_label(
                        r,
                        gc,
                        size,
                        scale,
                        &sk,
                        geometry.backup_remote_protocol,
                        language.pick("协议", "Protocol"),
                        "",
                        sk.ink,
                    );
                    combobox_value(
                        r,
                        gc,
                        geometry.backup_remote_protocol,
                        super::super::backup_protocol_label(view.backup_protocol, language),
                        sk.ink,
                    );
                }
                for (index, row) in
                    geometry.backup_remote_fields.iter().take(field_count).enumerate()
                {
                    if !visible(row.1, row.3) {
                        continue;
                    }
                    super::super::render::row_label(
                        r,
                        gc,
                        size,
                        scale,
                        &sk,
                        *row,
                        super::super::backup_remote_field_label(view.backup_protocol, index, language),
                        "",
                        sk.ink,
                    );
                    let (ix, iy, iw, ih) = super::super::sync_input_rect(*row, scale);
                    let max_cols = (((iw - s(24.0)) / cell_w) as usize).max(1);
                    let (text, placeholder, _) = super::super::backup_remote_input_display(view, index, max_cols);
                    let ink = if placeholder { sk.ink_dim } else { sk.ink };
                    r.draw_chrome_text(
                        size,
                        ix + s(12.0),
                        iy + (ih - cell_h) / 2.0,
                        ink,
                        &text,
                        gc,
                    );
                }
                if view.backup_protocol != crate::backup_remote::BackupProtocol::Off {
                    let actions = super::super::backup_remote_actions_rect(&geometry, scale, field_count);
                    if visible(actions.1, actions.3) {
                        let [push_rect, pull_rect] = super::super::sync_button_rects(actions, scale);
                        let captions = [
                            (push_rect, language.pick("备份到远程", "Back up now")),
                            (pull_rect, language.pick("恢复最新备份", "Restore latest")),
                        ];
                        for ((bx, by, bw, bh), caption) in captions {
                            let cols: usize =
                                caption.chars().map(|c| c.width().unwrap_or(1).max(1)).sum();
                            let ink = if view.backup_busy { sk.ink_dim } else { sk.ink };
                            r.draw_chrome_text(
                                size,
                                bx + (bw - cols as f32 * cell_w) / 2.0,
                                by + (bh - cell_h) / 2.0,
                                ink,
                                caption,
                                gc,
                            );
                        }
                    }
                }
            }
            // 状态行画在触发动作的那组控件下方（本地导出/恢复 → 清单卡尾；
            // 远程动作 → 远程按钮行下）。
            if let Some((status, error)) = &view.backup_status {
                let status_y = if view.backup_status_remote {
                    let actions = super::super::backup_remote_actions_rect(&geometry, scale, field_count);
                    actions.1 + actions.3 + s(8.0)
                } else {
                    let last = geometry.backup_rows[8];
                    last.1 + last.3 + s(8.0)
                };
                if visible(status_y, cell_h) {
                    r.draw_chrome_text(
                        size,
                        geometry.backup_actions.0,
                        status_y,
                        if *error {
                            Rgb::new(sk.danger.r, sk.danger.g, sk.danger.b)
                        } else {
                            sk.ink_dim
                        },
                        status,
                        gc,
                    );
                }
            }
}
