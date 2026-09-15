// Settings UI: Proxy page (SSH proxy mode, manual config, test).

use crate::display::color::Rgb;
use crate::display::ui::surface;
use crate::display::ui::text_field;
use crate::display::ui::theme::Skin;
use crate::display::ui::widgets;
use crate::display::{SettingsHit, SettingsDropdown, SizeInfo, UiLanguage};
use crate::display::settings::geometry::SettingsGeometry;
use crate::display::settings::view::SettingsView;
use crate::display::settings::settings_toggle_slot;
use crate::renderer::ui::{Rgba, UiQuad};
use crate::renderer::{GlyphCache, Renderer};
use unicode_width::UnicodeWidthChar;

pub(crate) fn push_proxy_quads(
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
    let combobox = |quads: &mut Vec<UiQuad>,
                    staged: &mut Vec<UiQuad>,
                    row, hot: bool, open: bool| {
        widgets::push_combobox(staged, widgets::combobox_rect(row, scale), scale, &sk, hot, open);
        for quad in staged.drain(..) { clip(quads, quad); }
    };
    widgets::push_combobox(
        &mut staged,
        super::super::ssh_proxy_mode_control(geometry.ssh_proxy_mode, scale),
        scale, &sk,
        view.hover == SettingsHit::SshProxyModeDropdown,
        view.dropdown == Some(SettingsDropdown::SshProxyMode),
    );
    for quad in staged.drain(..) { clip(quads, quad); }
    let cell_w = size.cell_width();
    let input_control = |quads: &mut Vec<UiQuad>, (ix, iy, iw, ih), index: usize| {
        let focused = view.ssh_proxy_focus == Some(index);
        let mut input = Vec::new();
        surface::push_input(&mut input, (ix, iy, iw, ih), scale, &sk, view.density, focused);
        if focused {
            let max_cols = (((iw - s(24.0)) / cell_w) as usize).max(1);
            let (display, placeholder, _, hidden) =
                super::super::ssh_proxy_input_display(view, index, max_cols);
            if !placeholder {
                text_field::push_cursor(&mut input, iy, ih, ix + s(12.0), &display,
                    &view.ssh_proxy_cursors[index].shifted(hidden), cell_w, scale, &sk);
            } else {
                text_field::push_cursor(&mut input, iy, ih, ix + s(12.0), "",
                    &view.ssh_proxy_cursors[index], cell_w, scale, &sk);
            }
        }
        for quad in input { clip(quads, quad); }
    };
    if view.ssh_proxy_mode == crate::ssh_proxy::ProxyMode::Custom {
        let (protocol, address) =
            super::super::ssh_proxy_manual_controls(geometry.ssh_proxy_expand, scale);
        widgets::push_combobox(
            &mut staged, protocol, scale, &sk,
            view.hover == SettingsHit::SshProxyProtocolDropdown,
            view.dropdown == Some(SettingsDropdown::SshProxyProtocol),
        );
        for quad in staged.drain(..) { clip(quads, quad); }
        input_control(quads, address, 0);
    }
    let banner = geometry.ssh_proxy_test;
    let corner = s(super::super::tokens::radius::OVERLAY);
    let mut banner_quads = Vec::new();
    surface::push_stroke(&mut banner_quads, banner, corner, scale, sk.hairline);
    banner_quads.push(UiQuad::solid(banner.0, banner.1, banner.2, banner.3, corner, sk.surface));
    let button = super::super::ssh_proxy_test_button(banner, scale);
    widgets::push_outline_button(&mut banner_quads, button, scale, &sk,
        view.hover == SettingsHit::SshProxyTest
            && !matches!(view.proxy_test_status, super::super::ProxyTestStatus::Running));
    for quad in banner_quads { clip(quads, quad); }
}

pub(crate) fn draw_proxy_text(
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
    let (gx, test_y, _, _) = geometry.ssh_proxy_test;
    let title_y = super::super::proxy_section_title_y(test_y, scale);
    if visible(title_y, title_h) {
        super::super::render::section_title(r, gc, size, scale, &sk, gx, title_y,
            language.pick("网络代理", "Network proxy"));
    }
    if visible(geometry.ssh_proxy_mode.1, geometry.ssh_proxy_mode.3) {
        super::super::render::row_label(r, gc, size, scale, &sk, geometry.ssh_proxy_mode,
            language.pick("代理方式", "Proxy setting"), "", sk.ink);
        let rect = super::super::ssh_proxy_mode_control(geometry.ssh_proxy_mode, scale);
        let tx = widgets::combobox_text_x(rect, scale);
        let right = widgets::combobox_text_right(rect, scale);
        let max_chars = ((right - tx).max(cell_w) / cell_w).floor().max(1.0) as usize;
        let value = super::super::truncate_tab_label(
            super::super::ssh_proxy_mode_label(view.ssh_proxy_mode, language), max_chars);
        r.draw_chrome_text(size, tx, rect.1 + (rect.3 - cell_h) / 2.0, sk.accent, &value, gc);
    }
    if view.ssh_proxy_mode == crate::ssh_proxy::ProxyMode::Custom {
        let row = geometry.ssh_proxy_expand;
        if visible(row.1, row.3) {
            super::super::render::row_label(r, gc, size, scale, &sk, row,
                language.pick("代理地址","Proxy address"), "", sk.ink);
            let (protocol, address) = super::super::ssh_proxy_manual_controls(row, scale);
            let tx = widgets::combobox_text_x(protocol, scale);
            let right = widgets::combobox_text_right(protocol, scale);
            let max_chars = ((right - tx).max(cell_w) / cell_w).floor().max(1.0) as usize;
            let value = super::super::truncate_tab_label(
                super::super::manual_proxy_protocol_label(view.ssh_proxy_protocol, language),
                max_chars);
            r.draw_chrome_text(size, tx, protocol.1 + (protocol.3 - cell_h) / 2.0, sk.accent, &value, gc);
            let (ix, iy, iw, ih) = address;
            let max_cols = (((iw - s(24.0)) / cell_w) as usize).max(1);
            let (text, placeholder, _, _) = super::super::ssh_proxy_input_display(view, 0, max_cols);
            r.draw_chrome_text(size, ix + s(12.0), iy + (ih - cell_h) / 2.0,
                if placeholder { sk.ink_dim } else { sk.ink }, &text, gc);
        }
    }
    let banner = geometry.ssh_proxy_test;
    if visible(banner.1, banner.3) {
        let button = super::super::ssh_proxy_test_button(banner, scale);
        let (status, status_ink) = match &view.proxy_test_status {
            super::super::ProxyTestStatus::Idle => (
                language.pick("测试当前设置是否可以访问网络",
                    "Test whether the current setting can access the network").to_owned(),
                sk.ink_dim,
            ),
            super::super::ProxyTestStatus::Running => (
                language.pick("正在通过当前设置测试网络…",
                    "Testing through the current setting…").to_owned(),
                sk.accent,
            ),
            super::super::ProxyTestStatus::Complete { outcome, elapsed_ms } => (
                language.proxy_test_message(outcome, *elapsed_ms),
                if outcome.is_success() {
                    Rgb::new(sk.ok.r, sk.ok.g, sk.ok.b)
                } else {
                    Rgb::new(sk.danger.r, sk.danger.g, sk.danger.b)
                },
            ),
        };
        let available = (button.0 - banner.0 - s(28.0)).max(cell_w);
        let max_chars = (available / cell_w).floor().max(1.0) as usize;
        let status = super::super::truncate_tab_label(&status, max_chars);
        r.draw_chrome_text(size, banner.0 + s(14.0), banner.1 + (banner.3 - cell_h) / 2.0,
            status_ink, &status, gc);
        let caption = if matches!(view.proxy_test_status, super::super::ProxyTestStatus::Running) {
            language.pick("测试中…", "Testing…")
        } else {
            language.pick("测试网络", "Test network")
        };
        let caption_cols: usize = caption.chars().map(|ch| ch.width().unwrap_or(1).max(1)).sum();
        r.draw_chrome_text(size,
            button.0 + (button.2 - caption_cols as f32 * cell_w) / 2.0,
            button.1 + (button.3 - cell_h) / 2.0,
            if matches!(view.proxy_test_status, super::super::ProxyTestStatus::Running) {
                sk.ink_faint
            } else {
                sk.ink_dim
            },
            caption, gc);
    }
}