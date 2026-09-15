// Settings UI: SSH page (saved hosts, hidden hosts, import config).

use crate::display::color::Rgb;
use crate::display::ui::surface;
use crate::display::ui::theme::Skin;
use crate::display::ui::{icons, os_icons, widgets};
use crate::display::ui::tokens;
use crate::display::{SettingsHit, SizeInfo, UiLanguage};
use crate::display::settings::geometry::SettingsGeometry;
use crate::display::settings::view::SettingsView;
use crate::display::settings::{settings_toggle_slot, STANDARD_ROW_ACTION_W};
use crate::renderer::ui::{Rgba, UiQuad};
use crate::renderer::{GlyphCache, Renderer};
use unicode_width::UnicodeWidthChar;

pub(crate) fn push_ssh_quads(
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
    let group_frame = |_quads: &mut Vec<UiQuad>, _first_row, _rows: usize| {};
    let action_button = |quads: &mut Vec<UiQuad>, row, logical_w: f32, hovered: bool| {
        let rect = super::super::row_action_rect(row, scale, logical_w);
        clip(
            quads,
            UiQuad::solid(
                rect.0, rect.1, rect.2, rect.3, rect.3 * 0.5,
                if hovered { sk.hover } else { sk.surface },
            ),
        );
    };
    for index in 0..geometry.ssh_host_count {
        let row = (
            geometry.ssh_host_row0.0,
            geometry.ssh_host_row0.1
                + index as f32 * (geometry.ssh_host_row_h + geometry.ssh_host_gap),
            geometry.ssh_host_row0.2,
            geometry.ssh_host_row_h,
        );
        let (rx, ry, rw, rh) = row;
        let row_hovered = matches!(
            view.hover,
            SettingsHit::SshHostRow(i)
                | SettingsHit::SshHostConnect(i)
                | SettingsHit::SshHostEdit(i)
                | SettingsHit::SshHostDelete(i) if i == index
        );
        if row_hovered {
            clip(quads, UiQuad::solid(rx, ry, rw, rh, s(tokens::radius::OVERLAY), sk.surface));
        }
        if row_hovered {
            let row_bg = surface::over(sk.surface, sk.panel);
            let connect = super::super::ssh_host_action_rect(row, scale, 0);
            let connect_hot = view.hover == SettingsHit::SshHostConnect(index);
            let mut button = Vec::new();
            widgets::push_outline_button(&mut button, connect, scale, &sk, connect_hot);
            for quad in button { clip(quads, quad); }
            for (action, icon) in [
                (1usize, icons::RowActionIcon::Edit),
                (2usize, icons::RowActionIcon::Delete),
            ] {
                let rect = super::super::ssh_host_action_rect(row, scale, action);
                let icon_hovered = match (action, view.hover) {
                    (1, SettingsHit::SshHostEdit(i)) => i == index,
                    (2, SettingsHit::SshHostDelete(i)) => i == index,
                    _ => false,
                };
                let (ink, cutout) = if icon_hovered {
                    clip(quads, UiQuad::solid(
                        rect.0, rect.1, rect.2, rect.3,
                        s(tokens::radius::CONTROL), sk.hover,
                    ));
                    (Rgba::opaque(sk.ink), surface::over(sk.hover, row_bg))
                } else {
                    (Rgba::opaque(sk.ink_faint), row_bg)
                };
                let mut ink_quads = Vec::new();
                icons::push_row_action_icon(&mut ink_quads, icon, rect, scale, ink, cutout);
                for quad in ink_quads { clip(quads, quad); }
            }
        }
    }
    let add = geometry.ssh_add_host;
    let add_hovered = view.hover == SettingsHit::SshAddHost;
    let mut add_button = Vec::new();
    widgets::push_outline_button(&mut add_button, add, scale, &sk, add_hovered);
    for quad in add_button { clip(quads, quad); }
    let add_icon_rect = if add.2 < s(96.0) { add } else { (add.0 + s(4.0), add.1, add.3, add.3) };
    let mut add_icon = Vec::new();
    let add_icon_rgb = if add_hovered { sk.icon_hover } else { sk.icon };
    icons::push_add(&mut add_icon, add_icon_rect, scale,
        Rgba::new(add_icon_rgb.r, add_icon_rgb.g, add_icon_rgb.b, 230));
    for quad in add_icon { clip(quads, quad); }
    group_frame(quads, geometry.ssh_import_config, 1);
    action_button(quads, geometry.ssh_import_config, STANDARD_ROW_ACTION_W,
        view.hover == SettingsHit::SshImportConfig);
    if geometry.hidden_host_count > 0 {
        group_frame(quads, geometry.hidden_host_row0, geometry.hidden_host_count);
        for index in 0..geometry.hidden_host_count {
            let mut rect = geometry.hidden_host_row0;
            rect.1 += index as f32 * rect.3;
            action_button(quads, rect, 80.0, view.hover == SettingsHit::RestoreHiddenSsh(index));
        }
    }
}

pub(crate) fn draw_ssh_text(
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
    let group_y = |row_y: f32| row_y - s(42.0);
    let (host_x, host_y, host_w, _host_h) = geometry.ssh_host_row0;
    if visible(group_y(host_y), title_h) {
        let available = (geometry.ssh_add_host.0 - host_x - s(8.0)).max(cell_w);
        let max_chars = (available / (cell_w * 1.2)).floor().max(1.0) as usize;
        let saved_hosts = super::super::truncate_tab_label(
            language.pick("已保存主机", "Saved hosts"), max_chars);
        super::super::render::section_title(
            r, gc, size, scale, &sk, host_x, group_y(host_y), &saved_hosts);
    }
    let add = geometry.ssh_add_host;
    if add.2 >= s(96.0) && visible(add.1, add.3) {
        r.draw_chrome_text(size, add.0 + s(36.0),
            widgets::centered_y(add.1, add.3, cell_h),
            if view.hover == SettingsHit::SshAddHost { sk.accent } else { sk.ink },
            language.pick("添加主机", "Add host"), gc);
    }
    for (index, host) in view.ssh_hosts.iter().enumerate() {
        let row = (
            host_x, host_y + index as f32 * (geometry.ssh_host_row_h + geometry.ssh_host_gap),
            host_w, geometry.ssh_host_row_h,
        );
        if !visible(row.1, row.3) { continue; }
        let detail_h = cell_h * 0.78;
        let title_y = widgets::centered_y(row.1, row.3, cell_h + detail_h);
        let icon = os_icons::resolve(Some(host.icon.as_str()));
        let icon_slot = (cell_h * 0.72).round();
        let icon_px = icon_slot * 0.82;
        let icon_mult = os_icons::scale_for(icon, size.cell_width(), icon_px);
        r.draw_chrome_text_scaled(size,
            row.0 + s(20.0) - icon_slot * 0.5 + (icon_slot - icon_px) * 0.5,
            widgets::centered_y(row.1, row.3, cell_h * icon_mult),
            icon_mult, sk.icon,
            icon.glyph.encode_utf8(&mut [0u8; 4]), gc);
        r.draw_chrome_text(size, row.0 + s(38.0), title_y, sk.ink, &host.label, gc);
        r.draw_ui_text(size, row.0 + s(38.0), title_y + cell_h * 0.95, 0.78, sk.ink_dim,
            nebula_terminal::term::cell::Flags::empty(), &host.destination, gc);
        if host.pinned {
            r.draw_chrome_text(size, row.0 + s(27.0), title_y, sk.accent, "\u{eab4}", gc);
        }
        let row_hovered = matches!(
            view.hover,
            SettingsHit::SshHostRow(i)
                | SettingsHit::SshHostConnect(i)
                | SettingsHit::SshHostEdit(i)
                | SettingsHit::SshHostDelete(i) if i == index
        );
        if row_hovered {
            let connect = super::super::ssh_host_action_rect(row, scale, 0);
            let label = language.pick("连接", "Connect");
            let cols = label.chars().map(|ch| ch.width().unwrap_or(1)).sum::<usize>();
            r.draw_chrome_text(size,
                connect.0 + (connect.2 - cols as f32 * cell_w) * 0.5,
                widgets::centered_y(connect.1, connect.3, cell_h),
                if view.hover == SettingsHit::SshHostConnect(index) { sk.accent } else { sk.ink },
                label, gc);
        }
    }
    if visible(geometry.ssh_import_config.1, geometry.ssh_import_config.3) {
        super::super::render::row_label(r, gc, size, scale, &sk, geometry.ssh_import_config,
            language.pick("导入 ~/.ssh/config", "Import ~/.ssh/config"), "", sk.ink);
        let action = super::super::row_action_rect(
            geometry.ssh_import_config, scale, STANDARD_ROW_ACTION_W);
        super::super::render::draw_button_label(r, gc, size, action,
            language.pick("立即刷新", "Refresh now"),
            if view.hover == SettingsHit::SshImportConfig { sk.accent } else { sk.ink_dim });
    }
    if geometry.hidden_host_count > 0 {
        let (hx, hy, hw, hh) = geometry.hidden_host_row0;
        if visible(group_y(hy), title_h) {
            super::super::render::section_title(r, gc, size, scale, &sk, hx, group_y(hy),
                language.pick("已隐藏主机", "Hidden hosts"));
        }
        for (index, host) in view.hidden_hosts.iter().enumerate() {
            let rect = (hx, hy + index as f32 * hh, hw, hh);
            if visible(rect.1, rect.3) {
                super::super::render::row_label(r, gc, size, scale, &sk, rect, host,
                    language.pick("恢复", "Restore"), sk.accent);
            }
        }
    }
}