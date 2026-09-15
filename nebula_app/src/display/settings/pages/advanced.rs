// Settings UI: Advanced page (session, tray, sync).

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

pub(crate) fn push_advanced_quads(
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
    let toggle = |quads: &mut Vec<UiQuad>,
                  staged: &mut Vec<UiQuad>,
                  row, hit: SettingsHit, on: bool, _hot: bool, _pressed: bool| {
        let motion = settings_toggle_slot(hit)
            .map_or_else(|| widgets::ToggleMotion::settled(on), |index| view.toggle_motion[index]);
        widgets::push_toggle(staged, row, scale, &sk, motion);
        for quad in staged.drain(..) { clip(quads, quad); }
    };
    group_frame(quads, geometry.keep_session, 4);
    row_hover(quads, geometry.keep_session, view.hover == SettingsHit::KeepSessionToggle);
    toggle(quads, &mut staged, geometry.keep_session, SettingsHit::KeepSessionToggle,
        view.keep_session, view.hover == SettingsHit::KeepSessionToggle,
        view.pressed == SettingsHit::KeepSessionToggle);
    row_hover(quads, geometry.restore_session, view.hover == SettingsHit::RestoreSessionToggle);
    toggle(quads, &mut staged, geometry.restore_session, SettingsHit::RestoreSessionToggle,
        view.restore_session, view.hover == SettingsHit::RestoreSessionToggle,
        view.pressed == SettingsHit::RestoreSessionToggle);
    row_hover(quads, geometry.resume_ai, view.hover == SettingsHit::ResumeAiToggle);
    toggle(quads, &mut staged, geometry.resume_ai, SettingsHit::ResumeAiToggle,
        view.resume_ai, view.hover == SettingsHit::ResumeAiToggle,
        view.pressed == SettingsHit::ResumeAiToggle);
    row_hover(quads, geometry.tray, view.hover == SettingsHit::TrayToggle);
    toggle(quads, &mut staged, geometry.tray, SettingsHit::TrayToggle,
        view.tray, view.hover == SettingsHit::TrayToggle,
        view.pressed == SettingsHit::TrayToggle);
    if super::super::SHOW_WEBDAV_SYNC_SETTINGS {
        group_frame(quads, geometry.sync_rows[0], 5);
        let cell_w = size.cell_width();
        for (index, row) in geometry.sync_rows.iter().enumerate() {
            row_hover(quads, *row, view.hover == SettingsHit::SyncInput(index));
            let (ix, iy, iw, ih) = super::super::sync_input_rect(*row, scale);
            let focused = view.sync_focus == Some(index);
            let border = if focused { sk.accent } else { sk.ink_dim };
            let border_alpha = if focused { 255 } else { 90 };
            clip(quads, UiQuad::solid(ix - s(1.0), iy - s(1.0), iw + s(2.0), ih + s(2.0),
                s(8.0), Rgba::new(border.r, border.g, border.b, border_alpha)));
            clip(quads, UiQuad::solid(ix, iy, iw, ih, s(7.0), sk.surface));
            if focused && super::super::caret_blink_on() {
                let max_cols = (((iw - s(24.0)) / cell_w) as usize).max(1);
                let (_, placeholder, cols) = super::super::sync_input_display(view, index, max_cols);
                let cols = if placeholder { 0 } else { cols };
                let caret_h = ih - s(10.0);
                clip(quads, UiQuad::solid(
                    (ix + s(12.0) + cols as f32 * cell_w).min(ix + iw - s(6.0)),
                    iy + (ih - caret_h) / 2.0,
                    (1.5 * scale).max(1.0), caret_h, 0.0,
                    Rgba::new(sk.accent.r, sk.accent.g, sk.accent.b, 255)));
            }
        }
        row_hover(quads, geometry.sync_auto_pull, view.hover == SettingsHit::SyncAutoPullToggle);
        toggle(quads, &mut staged, geometry.sync_auto_pull, SettingsHit::SyncAutoPullToggle,
            view.sync_auto_pull, view.hover == SettingsHit::SyncAutoPullToggle,
            view.pressed == SettingsHit::SyncAutoPullToggle);
        let [push_rect, pull_rect] = super::super::sync_button_rects(geometry.sync_actions, scale);
        for (rect, hit) in [(push_rect, SettingsHit::SyncPushButton),
            (pull_rect, SettingsHit::SyncPullButton)] {
            let (bx, by, bw, bh) = rect;
            let hot = view.hover == hit && !view.sync_busy;
            clip(quads, UiQuad::solid(bx - s(1.0), by - s(1.0), bw + s(2.0), bh + s(2.0),
                s(9.0), sk.hairline));
            clip(quads, UiQuad::solid(bx, by, bw, bh, s(8.0),
                if hot { sk.hover } else { sk.panel }));
        }
    }
    quads.extend(staged.drain(..));
}

pub(crate) fn draw_advanced_text(
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
) {            let (ax, ay, _, ah) = geometry.keep_session;
            if visible(group_y(ay), title_h) {
                super::super::render::section_title(
                    r,
                    gc,
                    size,
                    scale,
                    &sk,
                    ax,
                    group_y(ay),
                    language.pick("会话", "Sessions"),
                );
            }
            if visible(ay, ah) {
                // The switch (drawn in `push_quads`) carries the state; the
                // label says what closing a window keeps alive while it is ON.
                super::super::render::row_label(
                    r,
                    gc,
                    size,
                    scale,
                    &sk,
                    geometry.keep_session,
                    language.pick(
                        "关闭窗口后保留会话（后台驻留，可恢复对话）",
                        "Keep sessions after closing the window (resident and restorable)",
                    ),
                    "",
                    sk.ink,
                );
            }
            {
                let (_, ry, _, rh) = geometry.restore_session;
                if visible(ry, rh) {
                    // 关掉它就永远干净启动：session.json 照写不误（导出工作区
                    // 与崩溃诊断都靠它），只是开机不再回放。
                    super::super::render::row_label(
                        r,
                        gc,
                        size,
                        scale,
                        &sk,
                        geometry.restore_session,
                        language.pick(
                            "启动时恢复上次的标签（异常退出后同样恢复）",
                            "Restore last tabs on launch (also after a crash)",
                        ),
                        "",
                        sk.ink,
                    );
                }
            }
            {
                let (_, ry, _, rh) = geometry.resume_ai;
                if visible(ry, rh) {
                    // 关掉只是不敲 resume 命令：标签、分屏、目录照常恢复。
                    super::super::render::row_label(
                        r,
                        gc,
                        size,
                        scale,
                        &sk,
                        geometry.resume_ai,
                        language.pick(
                            "恢复时自动接续 AI 对话（claude / codex 自动 resume）",
                            "Resume AI conversations on restore (claude / codex)",
                        ),
                        "",
                        sk.ink,
                    );
                }
            }
            {
                let (_, ry, _, rh) = geometry.tray;
                if visible(ry, rh) {
                    // 关掉立即摘图标；agent 提醒仍有 toast 和任务栏闪烁兜底。
                    super::super::render::row_label(
                        r,
                        gc,
                        size,
                        scale,
                        &sk,
                        geometry.tray,
                        language.pick(
                            "常驻系统托盘图标（agent 等待输入时变色提醒）",
                            "System tray icon (turns amber when an agent needs you)",
                        ),
                        "",
                        sk.ink,
                    );
                }
            }

            if SHOW_WEBDAV_SYNC_SETTINGS {
                // ---- 同步（WebDAV）----
                let (sx, sy, ..) = geometry.sync_rows[0];
                if visible(group_y(sy), title_h) {
                    super::super::render::section_title(
                        r,
                        gc,
                        size,
                        scale,
                        &sk,
                        sx,
                        group_y(sy),
                        language.pick("同步（WebDAV）", "Sync (WebDAV)"),
                    );
                }
                let labels = [
                    language.pick("服务器文件 URL", "Server file URL"),
                    language.pick("用户名", "Username"),
                    language.pick("WebDAV 密码", "WebDAV password"),
                    language.pick("端到端口令", "End-to-end passphrase"),
                ];
                let cell_w = size.cell_width();
                let cell_h = size.cell_height();
                for (index, row) in geometry.sync_rows.iter().enumerate() {
                    if !visible(row.1, row.3) {
                        continue;
                    }
                    super::super::render::row_label(r, gc, size, scale, &sk, *row, labels[index], "", sk.ink);
                    let (ix, iy, iw, ih) = super::super::sync_input_rect(*row, scale);
                    let max_cols = (((iw - s(24.0)) / cell_w) as usize).max(1);
                    let (text, placeholder, _) = super::super::sync_input_display(view, index, max_cols);
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
                if visible(geometry.sync_auto_pull.1, geometry.sync_auto_pull.3) {
                    super::super::render::row_label(
                        r,
                        gc,
                        size,
                        scale,
                        &sk,
                        geometry.sync_auto_pull,
                        language.pick("启动时自动拉取", "Pull automatically on startup"),
                        "",
                        sk.ink,
                    );
                }
                let (_, by, _, bh) = geometry.sync_actions;
                if visible(by, bh + s(30.0)) {
                    let [push_rect, pull_rect] = super::super::sync_button_rects(geometry.sync_actions, scale);
                    let captions = [
                        (push_rect, language.pick("立即推送", "Push now")),
                        (pull_rect, language.pick("立即拉取", "Pull now")),
                    ];
                    for ((bx, byy, bw, bhh), caption) in captions {
                        let cols: usize =
                            caption.chars().map(|c| c.width().unwrap_or(1).max(1)).sum();
                        let ink = if view.sync_busy { sk.ink_dim } else { sk.ink };
                        r.draw_chrome_text(
                            size,
                            bx + (bw - cols as f32 * cell_w) / 2.0,
                            byy + (bhh - cell_h) / 2.0,
                            ink,
                            caption,
                            gc,
                        );
                    }
                    // 状态行：最近一次动作结果（错误红、成功淡墨）。
                    if let Some((message, error)) = &view.sync_status {
                        let ink = if *error {
                            Rgb::new(sk.danger.r, sk.danger.g, sk.danger.b)
                        } else {
                            sk.ink_dim
                        };
                        r.draw_chrome_text(size, sx, by + bh + s(8.0), ink, message, gc);
                    }
                }
            }
}
