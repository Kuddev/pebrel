//! Overlays: the confirm modal, the AI-fix bar, the SSH delete-undo toast,
//! the SSH-connect progress card and its hit-testing, the resize HUD, and the
//! IME preedit preview popup.

use std::cmp;
use std::num::NonZeroU32;

use unicode_width::UnicodeWidthChar;

use nebula_terminal::grid::Dimensions;
use nebula_terminal::index::{Column, Point};
use nebula_terminal::term::LineDamageBounds;
use nebula_terminal::term::cell::Flags;
use nebula_terminal::vte::ansi::{CursorShape, NamedColor};

use crate::config::UiConfig;
use crate::display::color::Rgb;
use crate::display::content::RenderableCursor;
use crate::display::cursor::IntoRects;
use crate::renderer::rects::{RenderLine, RenderRect};
use crate::renderer::ui::{Rgba, UiQuad};
use crate::string::{ShortenDirection, StrShortener};

use super::ssh_connect;
use super::ssh_ui::SSH_DELETE_UNDO_DURATION;
use super::text_path_model::truncate_tab_label;
use super::ui;
use super::{NebulaConfirm, SHORTENER, wrap_display_cols};

use super::Display;

impl Display {
    /// Centered modal for confirmations and mandatory setup gates.
    pub(super) fn draw_confirm_modal(&mut self) {
        let Some(confirm) = self.nebula_confirm.clone() else {
            self.nebula_confirm_buttons = None;
            return;
        };
        let size = self.ui_size_info();
        let scale = self.window.scale_factor as f32;
        let s = |v: f32| v * scale;
        let cell_w = size.cell_width();
        let cell_h = size.cell_height();

        // Same tokens as the settings shell (design discipline: one flat
        // surface, hairline stroke, semantic color only on the primary
        // action). Danger red for destructive closes, theme accent for paste.
        // All from the theme skin, so light themes get a pale card + dark ink.
        let sk = self.nebula_theme.skin();
        let accent = Rgba::new(sk.accent.r, sk.accent.g, sk.accent.b, 255);
        let txt = sk.ink;
        let dim = sk.ink_dim;

        let (title, body, danger) = match &confirm {
            NebulaConfirm::EnableBackgroundImageCoverChrome => (
                "让背景图覆盖窗口控件区域？".to_owned(),
                "背景图会延伸到标题栏、窗口按钮、Tab 与 SSH 侧栏下方，低对比度图片可能影响操作可见性；界面仍会保留最低不透明度保护。".to_owned(),
                false,
            ),
            NebulaConfirm::EnablePanelResize => (
                "开启侧栏拖拽调节？".to_owned(),
                "拖动左侧栏或右侧抽屉的宽度时，终端内容会跟随实时重排；在低性能设备或超大回滚缓冲下可能出现掉帧。拖动已按帧率与列宽双重节流，把左侧栏一路拖到最左即可收起。宽度会保存，此功能可随时关闭。".to_owned(),
                false,
            ),
            NebulaConfirm::InstallRequiredFont { .. } => (
                "建议安装终端字体".to_owned(),
                "未检测到 Maple Mono Nerd Font；缺少图标时可安装后重启 Nebula。".to_owned(),
                false,
            ),
            NebulaConfirm::ClosePane { process, .. } => (
                "关闭此分栏？".to_owned(),
                format!("{process} 仍在运行，关闭会中止它。"),
                true,
            ),
            NebulaConfirm::CloseTab { process, .. } => (
                "关闭此标签页？".to_owned(),
                format!("{process} 仍在运行，关闭会中止它。"),
                true,
            ),
            NebulaConfirm::CloseWindow { process } => (
                "关闭整个窗口？".to_owned(),
                format!("{process} 仍在运行，关闭会中止它。"),
                true,
            ),
            NebulaConfirm::Paste { lines, .. } => (
                format!("粘贴 {lines} 行文本？"),
                "多行粘贴会被 shell 逐行执行，请确认来源可信。".to_owned(),
                false,
            ),
            NebulaConfirm::DeleteSsh { host, from_config } => {
                let host = truncate_tab_label(host, 28);
                if *from_config {
                    (
                        format!("隐藏 SSH 主机 {host}？"),
                        "只从 Nebula 隐藏；~/.ssh/config 不会修改，保存的密码将在撤销期后清除。"
                            .to_owned(),
                        true,
                    )
                } else {
                    (
                        format!("删除 SSH 主机 {host}？"),
                        "会从主机列表移除，保存的 Windows 密码将在撤销期后清除。".to_owned(),
                        true,
                    )
                }
            },
            NebulaConfirm::DeleteSftp { entry } => (
                format!("删除远端项目 {}？", truncate_tab_label(&entry.name, 28)),
                if entry.kind == crate::ssh_sftp::SftpEntryKind::Directory {
                    "文件夹及其全部远端内容会被递归删除，此操作无法撤销。".to_owned()
                } else {
                    "远端文件会被永久删除，此操作无法撤销。".to_owned()
                },
                true,
            ),
            NebulaConfirm::DeleteFileTreePath { path, is_dir } => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                (
                    format!("删除 {}？", truncate_tab_label(&name, 28)),
                    if *is_dir {
                        "文件夹及其全部内容会移入回收站。".to_owned()
                    } else {
                        "文件会移入回收站。".to_owned()
                    },
                    true,
                )
            },
            NebulaConfirm::BackupPassphrase { restoring } => (
                if *restoring {
                    "输入恢复口令".to_owned()
                } else {
                    "设置备份口令".to_owned()
                },
                if *restoring {
                    "输入导出时使用的口令；认证通过后才会写入任何文件。".to_owned()
                } else {
                    "口令至少 8 个字符。Nebula 不会保存口令，丢失后无法恢复此备份。".to_owned()
                },
                false,
            ),
        };

        let is_backup_passphrase = matches!(confirm, NebulaConfirm::BackupPassphrase { .. });
        let body = if is_backup_passphrase {
            match &self.nebula_backup_status {
                Some((message, true)) => format!("{body} {message}"),
                _ => body,
            }
        } else {
            body
        };

        let text_w = |t: &str| -> f32 {
            let cols: usize = t.chars().map(|c| c.width().unwrap_or(1)).sum();
            cols as f32 * cell_w
        };

        // Buttons: right-aligned row, primary rightmost (Windows order). 文案
        // 统一"是 / 否"（2026-07-23 用户裁定）。
        //
        // 2026-07-27 用户反馈：Enter / Esc 一直生效，但按钮上只有"是""否"
        // 两个字，键位从没画出来——同文件的 SSH 撤销条却老实写着 Ctrl+Z，
        // 标准不一致。`can_dismiss()` 恒真，故两个键位都名副其实。
        //
        // 键位画成键帽（描边小方框）而不是裸文字：v0.5/v0.6 起 welcome 页的
        // 快捷键就是灰底药丸 + 亮墨的键帽（`welcome.rs` 的 `kbd`），用户记
        // 的"白色的框"就是它。那边是终端文本（ANSI + powerline 圆头字形），
        // 这里走 draw_ui + chrome text，因此使用本文件既有的惯用法
        // 手画：外圈描边 quad + 内层填充 quad，只露 1px 圆环。描边取按钮
        // 自己的墨色，深色主题下自然读作白框，浅色主题下是深框。
        let language = self.ui_language();
        let primary_label = language.pick("是", "Yes");
        let cancel_label = language.pick("否", "No");
        let primary_key = "Enter";
        let cancel_key = "Esc";
        let btn_h = s(34.0);
        let btn_pad = s(18.0);
        let btn_min_w = s(88.0);
        // Keycap: text plus breathing room, and a hair taller than the glyph so
        // the ring never clips ascenders. Gap sits between label and cap.
        let cap_pad = s(6.0);
        let cap_h = cell_h + s(6.0);
        let key_gap = s(8.0);
        let cap_w = |key: &str| text_w(key) + 2.0 * cap_pad;
        let btn_w = |label: &str, key: &str| -> f32 {
            (text_w(label) + key_gap + cap_w(key) + 2.0 * btn_pad).max(btn_min_w)
        };
        let primary_w = btn_w(primary_label, primary_key);
        let cancel_w = btn_w(cancel_label, cancel_key);

        // Card sized to title/buttons, clamped into the window, and capped at
        // 520 logical px: a long body WRAPS instead of stretching the card
        // into a full-width banner (2026-07-23 用户反馈"警告框太宽").
        let pad = s(26.0);
        let head_w = text_w(&title).max(primary_w + s(12.0) + cancel_w);
        let box_w = (head_w + 2.0 * pad).max(s(380.0)).min(s(520.0)).min(size.width() - s(32.0));
        let body_cols = (((box_w - 2.0 * pad) / cell_w).floor() as usize).max(8);
        let body_lines = wrap_display_cols(&body, body_cols);
        let line_h = cell_h + s(6.0);
        let body_h = body_lines.len() as f32 * line_h - s(6.0);
        let input_h = if is_backup_passphrase { s(38.0) } else { 0.0 };
        let input_space = if is_backup_passphrase { input_h + s(14.0) } else { 0.0 };
        let box_h = pad + cell_h + s(10.0) + body_h + input_space + s(24.0) + btn_h + pad * 0.75;
        let bx = ((size.width() - box_w) * 0.5).max(s(16.0));
        let by = ((size.height() - box_h) * 0.5).max(s(16.0));

        // 确认框是 Modal：它要求一个决策、有后果、必须应答，所以画遮罩。
        // 面板底、遮罩、外阴影、同心描边、圆角全部来自同一个配方，与命令
        // 面板/右键菜单共用——此前这里是手写的「遮罩 + 描边 + 填充」三件套，
        // 圆角 12/13 与别处的 8 对不上，而且**根本没有外阴影**：一个要求
        // 用户停下来应答的东西，却比随手开关的命令面板浮得还低。
        let mut quads = Vec::new();
        ui::surface::push_surface(
            &mut quads,
            (bx, by, box_w, box_h),
            (size.width(), size.height()),
            scale,
            &sk,
            self.nebula_density,
            ui::surface::Elevation::Modal,
            1.0,
        );

        let backup_input_rect = is_backup_passphrase.then(|| {
            (bx + pad, by + pad + cell_h + s(10.0) + body_h + s(14.0), box_w - 2.0 * pad, input_h)
        });
        if let Some(input_rect) = backup_input_rect {
            ui::surface::push_stroke(
                &mut quads,
                input_rect,
                s(ui::tokens::radius::CONTROL),
                scale,
                sk.hairline,
            );
            quads.push(UiQuad::solid(
                input_rect.0,
                input_rect.1,
                input_rect.2,
                input_rect.3,
                s(ui::tokens::radius::CONTROL),
                sk.input,
            ));
            if self.nebula_backup_passphrase_select_all.is_selected()
                && !self.nebula_backup_passphrase.is_empty()
            {
                quads.push(UiQuad::solid(
                    input_rect.0 + s(8.0),
                    input_rect.1 + s(6.0),
                    (self.nebula_backup_passphrase.chars().count() as f32 * cell_w)
                        .min(input_rect.2 - s(16.0)),
                    input_rect.3 - s(12.0),
                    ui::tokens::radius::CHIP * scale,
                    sk.accent_soft,
                ));
            }
        }

        // Button geometry (kept for the mouse hit-test).
        let btn_y = by + box_h - pad * 0.75 - btn_h;
        let primary_x = bx + box_w - pad + s(2.0) - primary_w;
        let cancel_x = primary_x - s(12.0) - cancel_w;
        let primary_rect = (primary_x, btn_y, primary_w, btn_h);
        let cancel_rect = (cancel_x, btn_y, cancel_w, btn_h);
        self.nebula_confirm_buttons = Some((primary_rect, cancel_rect));

        let primary_fill = if danger { sk.danger } else { accent };
        // Ink first: the keycap ring is derived from the ink it wraps, so both
        // buttons' text colors have to exist before the quads are built.
        let on_primary = if danger { Rgb::new(255, 244, 246) } else { sk.ink_on_accent };
        // Keycap geometry, shared by the ring quads and the glyph runs below.
        let cap_y = btn_y + (btn_h - cap_h) / 2.0;
        let cancel_cap_x = cancel_x + btn_pad + text_w(cancel_label) + key_gap;
        let primary_cap_x = primary_x + btn_pad + text_w(primary_label) + key_gap;

        // Cancel: quiet ghost button (hairline + faint fill).
        let control_r = s(ui::tokens::radius::CONTROL);
        ui::surface::push_stroke(&mut quads, cancel_rect, control_r, scale, sk.hairline);
        quads.push(UiQuad::solid(cancel_x, btn_y, cancel_w, btn_h, control_r, sk.panel));
        quads.push(UiQuad::solid(cancel_x, btn_y, cancel_w, btn_h, control_r, sk.surface));
        // Primary: the single loud element on the card.
        quads.push(UiQuad::solid(primary_x, btn_y, primary_w, btn_h, control_r, primary_fill));

        // Keycaps: 图8 键帽规范（2026-07-29）——与 Ctrl+K/设置页共用
        // `keycap::push_chip` 配方（hairline 圈 + panel/surface 叠底），
        // 不再按按钮墨色自造描边圈。该配方只用于中性底的取消键；中性
        // panel 在深色主题里近黑，放到 accent 主按钮上会读成一块突兀的
        // 深色（issue #35），主键帽因此改走 `push_chip_on_fill`：底与
        // 底边从按钮自己的墨（`on_primary`）派生，深浅主题都成立。
        let cap = |x: f32, key: &str| -> Vec<UiQuad> {
            let mut out = Vec::new();
            ui::keycap::push_chip(&mut out, &sk, x, cap_y, cap_w(key), cap_h, scale);
            out
        };
        quads.extend(cap(cancel_cap_x, cancel_key));
        ui::keycap::push_chip_on_fill(
            &mut quads,
            on_primary,
            primary_cap_x,
            cap_y,
            cap_w(primary_key),
            cap_h,
            scale,
        );
        self.renderer.draw_ui(&size, &quads);

        // Text: free-pixel chrome text (no opaque cell backgrounds), left
        // aligned like a native Windows dialog.
        let glyph_cache = &mut self.glyph_cache;
        let tx = bx + pad;
        self.renderer.draw_chrome_text(&size, tx, by + pad, txt, &title, glyph_cache);
        let btn_text_y = btn_y + (btn_h - cell_h) / 2.0;
        // Body wraps to the card's inner width; lines carry a small leading.
        let mut line_y = by + pad + cell_h + s(10.0);
        for line in &body_lines {
            self.renderer.draw_chrome_text(&size, tx, line_y, dim, line, glyph_cache);
            line_y += line_h;
        }
        self.renderer.draw_chrome_text(
            &size,
            cancel_x + btn_pad,
            btn_text_y,
            txt,
            cancel_label,
            glyph_cache,
        );
        if let Some(input_rect) = backup_input_rect {
            let max_cols = (((input_rect.2 - s(20.0)) / cell_w) as usize).max(1);
            let count = self.nebula_backup_passphrase.chars().count();
            let (masked, input_ink) = if count == 0 {
                (language.pick("输入口令", "Passphrase").to_owned(), sk.ink_faint)
            } else if count > max_cols {
                (format!("…{}", "•".repeat(max_cols.saturating_sub(1))), sk.ink)
            } else {
                ("•".repeat(count), sk.ink)
            };
            self.renderer.draw_chrome_text(
                &size,
                input_rect.0 + s(10.0),
                input_rect.1 + (input_rect.3 - cell_h) / 2.0,
                input_ink,
                &masked,
                glyph_cache,
            );
        }

        // Key text is centered in its cap. The cap shares the button's text
        // centerline by construction, so `btn_text_y` needs no adjustment.
        // Ink stays full strength: the ring already marks this run as a key,
        // dimming it too would push it under the contrast floor.
        self.renderer.draw_chrome_text(
            &size,
            cancel_cap_x + cap_pad,
            btn_text_y,
            txt,
            cancel_key,
            glyph_cache,
        );
        // Danger keeps pale ink (red is dark in both modes); the accent
        // button contrast flips with the theme.
        self.renderer.draw_chrome_text(
            &size,
            primary_x + btn_pad,
            btn_text_y,
            on_primary,
            primary_label,
            glyph_cache,
        );
        // 主键帽文字随键帽底走按钮墨（`on_primary`）：键帽底就是这支墨的
        // 低透明度洗色，满强度的同一支墨在其上必然可读；取消键帽仍是中性
        // chip + sk.ink。
        self.renderer.draw_chrome_text(
            &size,
            primary_cap_x + cap_pad,
            btn_text_y,
            on_primary,
            primary_key,
            glyph_cache,
        );
    }

    /// Bottom-center reversible-action bar for SSH deletion. Its action rect is
    /// published to input after layout, keeping hover/click geometry identical
    /// to the pixels on screen.
    /// 助手建议条（spec 001）：底部居中浮条，SSH 撤销条同款组件语言（中性
    /// 壳、accent/danger 只在 ✦/⚠ 一处，渐变预算不动）。Pending 一行"正在
    /// 分析"，Ready 是图标 + 命令 + 暗色解释 + 键位提示；一律只贴不执行。
    /// 撤销条在场时让位——它 8 秒自清，之后建议条自然浮现。
    pub(super) fn draw_ai_fix_bar(&mut self) {
        use crate::ai_assistant::AiFixState;
        if self.nebula_ssh_delete_undo.is_some() {
            return;
        }
        let Some(state) = self.nebula_ai_fix_bar.clone() else { return };

        let size = self.ui_size_info();
        let scale = self.window.scale_factor as f32;
        let s = |value: f32| value * scale;
        let cell_w = size.cell_width();
        let cell_h = size.cell_height();
        let sk = self.nebula_theme.skin();
        let language = self.ui_language();
        let text_cols =
            |text: &str| -> usize { text.chars().map(|ch| ch.width().unwrap_or(1).max(1)).sum() };

        let accent = Rgba::new(sk.accent.r, sk.accent.g, sk.accent.b, 255);
        let (icon, icon_color, command, explain, hint) = match &state {
            AiFixState::Pending { .. } => (
                "✦",
                accent,
                language.pick("正在分析失败原因…", "Analyzing failure…").to_owned(),
                String::new(),
                String::new(),
            ),
            AiFixState::Ready { fix, .. } => (
                if fix.danger { "⚠" } else { "✦" },
                if fix.danger { sk.danger } else { accent },
                fix.command.clone(),
                fix.explain.clone(),
                language.pick("Ctrl+. 贴入 · Esc 关闭", "Ctrl+. paste · Esc dismiss").to_owned(),
            ),
        };

        // Budget: icon + command are non-negotiable; the explain is the first
        // thing dropped, then the command itself is HEAD-truncated (unlike
        // paths, a command's identity lives at its start).
        let pad = s(14.0);
        let gap = s(10.0);
        let max_w = size.width() - s(24.0);
        let fixed = pad * 2.0 + cell_w * 2.0 + gap + text_cols(&hint) as f32 * cell_w;
        let cmd_budget = (((max_w - fixed) / cell_w) as usize).max(12);
        let command = truncate_tab_label(&command, cmd_budget.min(96));
        let explain_budget =
            cmd_budget.saturating_sub(text_cols(&command)).saturating_sub(3).min(60);
        let explain =
            if text_cols(&explain) + 8 > explain_budget { String::new() } else { explain };

        let mut content_cols = 2 + text_cols(&command);
        if !explain.is_empty() {
            content_cols += 3 + text_cols(&explain);
        }
        if !hint.is_empty() {
            content_cols += 2 + text_cols(&hint);
        }
        let bar_h = s(44.0).max(cell_h + s(12.0));
        let bar_w = (pad * 2.0 + content_cols as f32 * cell_w).max(s(320.0)).min(max_w);
        let bar_x = (size.width() - bar_w) * 0.5;
        let bar_y = size.height() - bar_h - s(18.0);

        // 通知条是 Menu 层级的浮层：贴着窗口底边、不阻断交互，靠真外阴影
        // 与内容分层。此前这里是 `UiQuad::glow` 冒充阴影——glow 向外扩散
        // 亮度而不是压暗，在浅色主题上只会让条子四周发灰。
        let mut quads = Vec::new();
        ui::surface::push_surface(
            &mut quads,
            (bar_x, bar_y, bar_w, bar_h),
            (size.width(), size.height()),
            scale,
            &sk,
            self.nebula_density,
            ui::surface::Elevation::Menu,
            1.0,
        );
        self.renderer.draw_ui(&size, &quads);

        let text_y = bar_y + (bar_h - cell_h) * 0.5;
        let mut x = bar_x + pad;
        let gc = &mut self.glyph_cache;
        self.renderer.draw_chrome_text(
            &size,
            x,
            text_y,
            Rgb::new(icon_color.r, icon_color.g, icon_color.b),
            icon,
            gc,
        );
        x += cell_w * 2.0;
        self.renderer.draw_chrome_text(&size, x, text_y, sk.ink_strong, &command, gc);
        x += text_cols(&command) as f32 * cell_w;
        if !explain.is_empty() {
            self.renderer.draw_chrome_text(
                &size,
                x + cell_w,
                text_y,
                sk.ink_dim,
                &format!("— {explain}"),
                gc,
            );
            x += (3 + text_cols(&explain)) as f32 * cell_w;
        }
        if !hint.is_empty() {
            let hint_x = (bar_x + bar_w - pad - text_cols(&hint) as f32 * cell_w).max(x + gap);
            self.renderer.draw_chrome_text(&size, hint_x, text_y, sk.ink_dim, &hint, gc);
        }
    }

    pub(super) fn draw_ssh_delete_undo(&mut self) {
        let Some(undo) = self.nebula_ssh_delete_undo.as_ref() else {
            self.nebula_ssh_delete_undo_rect = None;
            self.nebula_ssh_delete_undo_hover = false;
            return;
        };
        if undo.started_at.elapsed() >= SSH_DELETE_UNDO_DURATION {
            self.expire_ssh_delete_undo();
            return;
        }

        let size = self.ui_size_info();
        let scale = self.window.scale_factor as f32;
        let s = |value: f32| value * scale;
        let cell_w = size.cell_width();
        let cell_h = size.cell_height();
        let sk = self.nebula_theme.skin();

        let fixed_cols = 20usize;
        let host_budget = (((size.width() - s(300.0)).max(cell_w * 8.0) / cell_w) as usize)
            .saturating_sub(fixed_cols)
            .max(8);
        let host = truncate_tab_label(&undo.host, host_budget.min(28));
        let message = if undo.from_config {
            format!("已隐藏 {host}（SSH config 未修改）")
        } else {
            format!("已移除 {host}")
        };
        let hint = "Ctrl+Z";
        let action = "撤销";
        let text_cols =
            |text: &str| -> usize { text.chars().map(|ch| ch.width().unwrap_or(1).max(1)).sum() };

        let pad = s(14.0);
        let gap = s(12.0);
        let action_w = s(76.0);
        let bar_h = s(48.0).max(cell_h + s(12.0));
        let content_w = (text_cols(&message) + text_cols(hint) + 2) as f32 * cell_w;
        let bar_w =
            (pad * 2.0 + content_w + gap + action_w).max(s(360.0)).min(size.width() - s(24.0));
        let bar_x = (size.width() - bar_w) * 0.5;
        let bar_y = size.height() - bar_h - s(18.0);
        let action_rect =
            (bar_x + bar_w - pad - action_w, bar_y + (bar_h - s(34.0)) * 0.5, action_w, s(34.0));
        self.nebula_ssh_delete_undo_rect = Some(action_rect);

        // 撤销条同上：Menu 层级的浮层配方，真外阴影而不是 glow。
        let mut quads = Vec::new();
        ui::surface::push_surface(
            &mut quads,
            (bar_x, bar_y, bar_w, bar_h),
            (size.width(), size.height()),
            scale,
            &sk,
            self.nebula_density,
            ui::surface::Elevation::Menu,
            1.0,
        );
        quads.push(UiQuad::solid(
            action_rect.0,
            action_rect.1,
            action_rect.2,
            action_rect.3,
            s(ui::tokens::radius::CONTROL),
            if self.nebula_ssh_delete_undo_hover { sk.hover_strong } else { sk.surface },
        ));
        if self.nebula_ssh_delete_undo_hover {
            quads.push(UiQuad::solid(
                action_rect.0,
                action_rect.1 + action_rect.3 - s(2.0),
                action_rect.2,
                s(2.0),
                s(1.0),
                Rgba::new(sk.accent.r, sk.accent.g, sk.accent.b, 220),
            ));
        }
        self.renderer.draw_ui(&size, &quads);

        let text_y = bar_y + (bar_h - cell_h) * 0.5;
        let message_x = bar_x + pad;
        self.renderer.draw_chrome_text(
            &size,
            message_x,
            text_y,
            sk.ink,
            &message,
            &mut self.glyph_cache,
        );
        let hint_x = action_rect.0 - gap - text_cols(hint) as f32 * cell_w;
        self.renderer.draw_chrome_text(
            &size,
            hint_x,
            text_y,
            sk.ink_faint,
            hint,
            &mut self.glyph_cache,
        );
        let action_x = action_rect.0 + (action_rect.2 - text_cols(action) as f32 * cell_w) * 0.5;
        let action_y = action_rect.1 + (action_rect.3 - cell_h) * 0.5;
        self.renderer.draw_chrome_text_styled(
            &size,
            action_x,
            action_y,
            if self.nebula_ssh_delete_undo_hover { sk.ink_strong } else { sk.accent },
            nebula_terminal::term::cell::Flags::BOLD,
            action,
            &mut self.glyph_cache,
        );
    }

    /// Draw the window chrome and present the accumulated frame.
    /// Overlay a transient, fading "cols × rows" HUD centered in the window,
    /// shown briefly after a resize (a resize overlay HUD). Keeps requesting
    /// redraws until it fades out, then clears itself.
    /// 每帧同步聚焦 pane 的身份。连接卡片据此决定画在哪个 pane 里——
    /// `nebula_pane_view` 只给几何，不给身份。
    pub fn set_focused_pane(&mut self, pane: u64) {
        self.nebula_focused_pane = pane;
    }

    /// 后台 SSH runtime 上报的连接阶段。
    ///
    /// `Ready` 直接移除状态：卡片退场，持续重绘随之停止，不会留下一个连完
    /// 还在后台跑粒子的 tab。
    pub fn ssh_connect_stage(
        &mut self,
        pane: u64,
        destination: String,
        stage: crate::ssh_session::SshStage,
    ) {
        if matches!(stage, crate::ssh_session::SshStage::Ready) {
            self.nebula_ssh_connect.remove(&pane);
            return;
        }
        match self.nebula_ssh_connect.entry(pane) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut().set_stage(stage),
            std::collections::hash_map::Entry::Vacant(entry) => {
                // 只有 `Resolve` 能开一张新卡片。`Ready` 时状态已被移除，若
                // 任何后续阶段都能重建，一次会话中途断线就会让连接卡片在一
                // 个用了半天的终端上凭空复活。
                if matches!(stage, crate::ssh_session::SshStage::Resolve) {
                    entry.insert(ssh_connect::SshConnectState::new(destination));
                }
            },
        }
    }

    /// pane 关闭时丢弃它的连接状态。
    pub fn forget_ssh_connect(&mut self, pane: u64) {
        self.nebula_ssh_connect.remove(&pane);
    }

    /// 卡片当前占据的矩形 = 聚焦 pane 的内容区。绘制与命中共用它，两者不会
    /// 漂移。
    fn ssh_connect_rect(&self) -> (f32, f32, f32, f32) {
        let view = self.pane_view();
        (
            view.padding_x(),
            view.padding_y(),
            view.width() - view.padding_x() - view.padding_right(),
            view.height() - view.padding_y() - view.padding_bottom(),
        )
    }

    /// 聚焦 pane 是否正被连接卡片接管（遮罩已经在画了）。
    pub fn ssh_connect_active(&self) -> bool {
        self.nebula_ssh_connect.get(&self.nebula_focused_pane).is_some_and(|state| state.visible())
    }

    /// 遮罩盖住整个 pane，所以卡片在场时 pane 内的一切点击都归卡片，不能
    /// 漏进终端去起拖选——侧栏拖拽残影那个 bug 的同类。
    pub fn ssh_connect_covers(&self, x: f32, y: f32) -> bool {
        self.ssh_connect_active() && ssh_connect::covers(self.ssh_connect_rect(), x, y)
    }

    pub fn ssh_connect_hit(&self, x: f32, y: f32) -> ssh_connect::SshConnectHit {
        let Some(state) = self.nebula_ssh_connect.get(&self.nebula_focused_pane) else {
            return ssh_connect::SshConnectHit::None;
        };
        if !state.visible() {
            return ssh_connect::SshConnectHit::None;
        }
        ssh_connect::hit_test(
            state,
            &self.ui_size_info(),
            self.ssh_connect_rect(),
            self.window.scale_factor as f32,
            self.nebula_language,
            self.nebula_density,
            x,
            y,
        )
    }

    /// 更新悬停并返回是否需要重绘。
    pub fn ssh_connect_set_hover(&mut self, hit: ssh_connect::SshConnectHit) -> bool {
        let pane = self.nebula_focused_pane;
        self.nebula_ssh_connect.get_mut(&pane).is_some_and(|state| state.set_hover(hit))
    }

    /// Logs 折叠是纯显示状态，就地处理；其余动作要关 pane 或重连，交给
    /// `window_context`。
    pub fn ssh_connect_toggle_logs(&mut self) {
        let pane = self.nebula_focused_pane;
        if let Some(state) = self.nebula_ssh_connect.get_mut(&pane) {
            state.toggle_logs();
        }
    }

    /// 聚焦 pane 的连接目标，供"重试"重新发起同一个连接。
    pub fn ssh_connect_destination(&self) -> Option<String> {
        self.nebula_ssh_connect
            .get(&self.nebula_focused_pane)
            .map(|state| state.destination().to_owned())
    }

    /// SSH 连接卡片：星云轨道 + 粒子流，画在聚焦 pane 内的浮层。
    pub(super) fn draw_ssh_connect(&mut self) {
        let pane = self.nebula_focused_pane;
        if !self.nebula_ssh_connect.contains_key(&pane) {
            return;
        }
        let delta = self.nebula_ui_anims.frame().delta;
        // 借用分离：绘制要同时摸 renderer 与 glyph_cache，先把状态摘出来。
        let mut states = std::mem::take(&mut self.nebula_ssh_connect);
        if let Some(state) = states.get_mut(&pane) {
            state.step(delta);
            if state.visible() {
                let size = self.ui_size_info();
                let scale = self.window.scale_factor as f32;
                let view = self.pane_view();
                // pane 的内容矩形：padding 编码了 pane 在窗口里的位置，
                // 分屏时左右两半的 padding 是非对称的。
                let rect = (
                    view.padding_x(),
                    view.padding_y(),
                    view.width() - view.padding_x() - view.padding_right(),
                    view.height() - view.padding_y() - view.padding_bottom(),
                );
                let mut quads = Vec::new();
                // 遮罩用 pane 的真实底色，这样卡片浮在一块与终端同色的板上，
                // 而不是凭空多出一层灰。
                let bg = self.nebula_background.unwrap_or(self.colors[NamedColor::Background]);
                let backdrop = crate::renderer::ui::Rgba::opaque(bg);
                let language = self.nebula_language;
                ssh_connect::push_quads(
                    state,
                    &self.nebula_theme,
                    &mut quads,
                    &size,
                    rect,
                    scale,
                    language,
                    self.nebula_density,
                    backdrop,
                );
                self.renderer.draw_ui(&size, &quads);
                let glyph_cache = &mut self.glyph_cache;
                ssh_connect::draw_text(
                    state,
                    &self.nebula_theme,
                    language,
                    &mut self.renderer,
                    glyph_cache,
                    &size,
                    rect,
                    scale,
                    self.nebula_density,
                );
            }
            // 门槛期内也要保持帧循环，否则永远到不了该显示的那一帧。
            // 失败态不再有动画，交给事件驱动即可。
            if !state.failed() {
                self.window.request_redraw();
            }
        }
        self.nebula_ssh_connect = states;
    }

    pub(super) fn draw_resize_hud(&mut self) {
        let Some(mut hud) = self.nebula_resize_hud else { return };
        hud.opacity.step(self.nebula_ui_anims.frame());
        if !hud.opacity.is_active() {
            self.nebula_resize_hud = None;
            return;
        }
        self.nebula_resize_hud = Some(hud);
        let cols = hud.columns;
        let rows = hud.rows;
        let fade = hud.opacity.value().clamp(0.0, 1.0);

        // UI-anchored metrics: the HUD is chrome, so its box and label must
        // not inflate with the terminal zoom it is reporting.
        let size = self.ui_size_info();
        let scale = self.window.scale_factor as f32;
        let cw = size.cell_width();
        let ch = size.cell_height();

        let text = format!("{cols} × {rows}");
        let text_cols: usize = text.chars().map(|c| c.width().unwrap_or(1)).sum();

        // Centered translucent rounded box (fades out), skinned by the theme
        // so it reads as chrome on light panels too.
        let sk = self.nebula_theme.skin();
        let hud_rgb = Rgb::new(sk.panel.r, sk.panel.g, sk.panel.b);
        let pad = 12.0 * scale;
        let box_w = text_cols as f32 * cw + 2.0 * pad;
        let box_h = ch + 2.0 * pad;
        let box_x = ((size.width() - box_w) * 0.5).max(0.0);
        let box_y = ((size.height() - box_h) * 0.5).max(0.0);
        let bg = Rgba::new(hud_rgb.r, hud_rgb.g, hud_rgb.b, 0).with_alpha(0.85 * fade);
        let quad = UiQuad::solid(box_x, box_y, box_w, box_h, 8.0 * scale, bg);
        self.renderer.draw_ui(&size, &[quad]);

        // The label shares the box's pixel coordinate system — the old
        // grid-cell placement centered on the TERMINAL area (whose origin
        // carries the asymmetric sidebar padding), so the text drifted out of
        // the window-centered box whenever the sidebar was open. Ink fades
        // with the box by mixing toward the panel color.
        let mix = |a: u8, b: u8| (a as f32 * fade + b as f32 * (1.0 - fade)).round() as u8;
        let ink = Rgb::new(
            mix(sk.ink_strong.r, hud_rgb.r),
            mix(sk.ink_strong.g, hud_rgb.g),
            mix(sk.ink_strong.b, hud_rgb.b),
        );
        let glyph_cache = &mut self.glyph_cache;
        self.renderer.draw_chrome_text(&size, box_x + pad, box_y + pad, ink, &text, glyph_cache);

        // Keep the frame loop alive so the HUD animates out.
        self.window.request_redraw();
    }

    #[inline(never)]
    pub(super) fn draw_ime_preview(
        &mut self,
        point: Point<usize>,
        fg: Rgb,
        bg: Rgb,
        rects: &mut Vec<RenderRect>,
        config: &UiConfig,
    ) {
        let preedit = match self.ime.preedit() {
            Some(preedit) => preedit,
            None => {
                // In case we don't have preedit, just set the popup point.
                self.window.update_ime_position(point, &self.size_info);
                return;
            },
        };

        let num_cols = self.size_info.columns();

        // Get the visible preedit.
        let visible_text: String = match (preedit.cursor_byte_offset, preedit.cursor_end_offset) {
            (Some(byte_offset), Some(end_offset)) if end_offset.0 > num_cols => StrShortener::new(
                &preedit.text[byte_offset.0..],
                num_cols,
                ShortenDirection::Right,
                Some(SHORTENER),
            ),
            _ => {
                StrShortener::new(&preedit.text, num_cols, ShortenDirection::Left, Some(SHORTENER))
            },
        }
        .collect();

        let visible_len = visible_text.chars().count();

        let end = cmp::min(point.column.0 + visible_len, num_cols);
        let start = end.saturating_sub(visible_len);

        let start = Point::new(point.line, Column(start));
        let end = Point::new(point.line, Column(end - 1));

        let glyph_cache = &mut self.glyph_cache;
        let metrics = glyph_cache.font_metrics();

        self.renderer.draw_string(
            start,
            fg,
            bg,
            visible_text.chars(),
            &self.size_info,
            glyph_cache,
        );

        // Damage preedit inside the terminal viewport.
        if point.line < self.size_info.screen_lines() {
            let damage = LineDamageBounds::new(start.line, 0, num_cols);
            self.damage_tracker.frame().damage_line(damage);
            self.damage_tracker.next_frame().damage_line(damage);
        }

        // Add underline for preedit text.
        let underline = RenderLine { start, end, color: fg };
        rects.extend(underline.rects(Flags::UNDERLINE, &metrics, &self.size_info));

        let ime_popup_point = match preedit.cursor_end_offset {
            Some(cursor_end_offset) => {
                // Use hollow block when multiple characters are changed at once.
                let (shape, width) = if let Some(width) =
                    NonZeroU32::new((cursor_end_offset.0 - cursor_end_offset.1) as u32)
                {
                    (CursorShape::HollowBlock, width)
                } else {
                    (CursorShape::Beam, NonZeroU32::new(1).unwrap())
                };

                let cursor_column = Column(
                    (end.column.0 as isize - cursor_end_offset.0 as isize + 1).max(0) as usize,
                );
                let cursor_point = Point::new(point.line, cursor_column);
                let cursor = RenderableCursor::new(cursor_point, shape, fg, width);
                rects.extend(cursor.rects(&self.size_info, config.cursor.thickness()));
                cursor_point
            },
            _ => end,
        };

        self.window.update_ime_position(ime_popup_point, &self.size_info);
    }
}
