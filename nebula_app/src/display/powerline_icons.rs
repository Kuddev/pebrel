//! Powerline icon types, pixel decoding and powerline drawing helpers.

use nebula_terminal::index::Point;

use super::Display;
use super::AiLogo;
use super::prepare_ai_logo_texture;
use crate::display::color::Rgb;
use crate::display::SizeInfo;
use crate::renderer::ui::{Gradient, Rgba, UiQuad};

/// Texture ids for chrome logos live far above the inline-image counter
/// (which starts at 1), so the two id spaces can share the renderer cache.
pub(super) const AI_LOGO_ID_BASE: u64 = 1 << 62;

#[derive(Debug, Clone, Copy)]
pub(super) enum NebulaPowerlineIconKind {
    Folder,
    GitBranch,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct NebulaPowerlineIcon {
    pub(super) kind: NebulaPowerlineIconKind,
    pub(super) point: Point<usize>,
}

/// Remove one destination while recording exactly enough list state for Undo.
/// Kept independent from rendering and Credential Manager so the destructive
/// state transition can be regression-tested without touching real secrets.
///
/// Only config-sourced aliases go to the hidden list: Nebula never edits
/// `~/.ssh/config`, so hiding is the strongest "delete" available for them.
/// Nebula-managed hosts are removed outright — parking them in the hidden
/// section made deletion read as a rename to "hidden".
pub(super) fn remove_ssh_host_from_lists(
    host: &str,
    from_config: bool,
    saved: &mut Vec<String>,
    pinned: &mut Vec<String>,
    hidden: &mut Vec<String>,
) -> (Option<usize>, Option<usize>, bool) {
    let saved_index = saved.iter().position(|entry| entry == host);
    let pinned_index = pinned.iter().position(|entry| entry == host);
    let was_hidden = hidden.iter().any(|entry| entry == host);
    saved.retain(|entry| entry != host);
    pinned.retain(|entry| entry != host);
    if from_config && !was_hidden {
        hidden.push(host.to_owned());
    }
    (saved_index, pinned_index, was_hidden)
}

pub(super) fn restore_ssh_host_to_lists(
    host: &str,
    saved_index: Option<usize>,
    pinned_index: Option<usize>,
    was_hidden: bool,
    saved: &mut Vec<String>,
    pinned: &mut Vec<String>,
    hidden: &mut Vec<String>,
) {
    saved.retain(|entry| entry != host);
    if let Some(index) = saved_index {
        saved.insert(index.min(saved.len()), host.to_owned());
    }
    pinned.retain(|entry| entry != host);
    if let Some(index) = pinned_index {
        pinned.insert(index.min(pinned.len()), host.to_owned());
    }
    if !was_hidden {
        hidden.retain(|entry| entry != host);
    }
}

/// Log replay commands can contain terminal query sequences captured from a
/// different process. Replying writes those answers into the shell's stdin,
/// where they become the next command after the replay process exits.
pub(crate) fn replays_untrusted_terminal_output(line: &str) -> bool {
    let words: Vec<String> = line
        .split_whitespace()
        .take(4)
        .map(|word| word.trim_matches(['"', '\'']).to_ascii_lowercase())
        .collect();
    matches!(
        words.as_slice(),
        [docker, logs, ..] if docker == "docker" && logs == "logs"
    ) || matches!(
        words.as_slice(),
        [docker, compose, logs, ..]
            if docker == "docker" && compose == "compose" && logs == "logs"
    ) || matches!(
        words.as_slice(),
        [podman, logs, ..] if podman == "podman" && logs == "logs"
    ) || matches!(
        words.as_slice(),
        [kubectl, logs, ..] if kubectl == "kubectl" && logs == "logs"
    ) || matches!(words.as_slice(), [journalctl, ..] if journalctl == "journalctl")
}

impl Display {
    /// Decoded (and theme-tinted) pixels for an AI brand logo, plus a stable
    /// texture id for the renderer's inline cache. Decode + tint run once per
    /// (logo, ink); the GPU upload happens lazily inside the renderer.
    fn ai_logo_pixels(
        &mut self,
        logo: AiLogo,
        ink: Rgb,
        target_size: u32,
    ) -> Option<(u64, std::sync::Arc<Vec<u8>>, (u32, u32))> {
        // Color assets keep their source colors. Grok ships official dark
        // and light marks, selected to match the chrome ink without tinting.
        let grok_uses_light_mark =
            u32::from(ink.r) * 299 + u32::from(ink.g) * 587 + u32::from(ink.b) * 114 >= 128_000;
        let key = match logo {
            AiLogo::Grok if grok_uses_light_mark => (logo, [255, 255, 255], target_size),
            AiLogo::OpenAi | AiLogo::OpenCode | AiLogo::Pi => {
                (logo, [ink.r, ink.g, ink.b], target_size)
            },
            _ => (logo, [0, 0, 0], target_size),
        };
        if let Some(cached) = self.nebula_ai_logo_cache.get(&key) {
            return Some(cached.clone());
        }
        let bytes = logo.png(grok_uses_light_mark);
        let (width, height, mut rgba) = match crate::renderer::image::decode_png_bytes(bytes) {
            Ok(decoded) => decoded,
            Err(err) => {
                log::warn!("failed to decode embedded AI logo: {err}");
                return None;
            },
        };
        logo.tint_pixels(&mut rgba, [ink.r, ink.g, ink.b]);
        let (rgba, width, height) = prepare_ai_logo_texture(&rgba, width, height, target_size);
        let id = AI_LOGO_ID_BASE + self.nebula_ai_logo_cache.len() as u64;
        let entry = (id, std::sync::Arc::new(rgba), (width, height));
        self.nebula_ai_logo_cache.insert(key, entry.clone());
        Some(entry)
    }

    /// Decoded pixels for a full-color shell icon (128x128 PNG embedded from
    /// extra/shell-icons), plus a stable texture id for the renderer's inline
    /// cache. Decode runs once per shell id; the GPU upload happens lazily
    /// inside the renderer. Returns `None` when the id has no brand asset.
    fn shell_icon_pixels(
        &mut self,
        shell_id: &str,
    ) -> Option<(u64, std::sync::Arc<Vec<u8>>, (u32, u32))> {
        if let Some(cached) = self.nebula_shell_icon_cache.get(shell_id) {
            return Some(cached.clone());
        }
        let bytes = crate::shell_detect::color_icon_png(shell_id)?;
        let (width, height, rgba) = match crate::renderer::image::decode_png_bytes(bytes) {
            Ok(decoded) => decoded,
            Err(err) => {
                log::warn!("failed to decode shell icon for {shell_id}: {err}");
                return None;
            },
        };
        // Shell icons ship in brand colors and are used as-is (no tint).
        let id = AI_LOGO_ID_BASE + 1000 + self.nebula_shell_icon_cache.len() as u64;
        let entry = (id, std::sync::Arc::new(rgba), (width, height));
        self.nebula_shell_icon_cache.insert(shell_id.to_owned(), entry.clone());
        Some(entry)
    }

    fn draw_powerline_icons(&mut self, icons: &[NebulaPowerlineIcon], view: SizeInfo) {
        if icons.is_empty() {
            return;
        }

        let size = view;
        let cell_w = size.cell_width();
        let cell_h = size.cell_height();
        let pad_x = size.padding_x();
        let pad_y = size.padding_y();
        let palette = self.nebula_theme.palette();
        let folder_color = Rgb::new(palette.edge_r.r, palette.edge_r.g, palette.edge_r.b);
        let branch_color = Rgb::new(palette.edge_l.r, palette.edge_l.g, palette.edge_l.b);

        let mut quads = Vec::with_capacity(icons.len() * 8);
        for icon in icons {
            if icon.point.line >= size.screen_lines() {
                continue;
            }

            let x = pad_x + icon.point.column.0 as f32 * cell_w;
            let y = pad_y + icon.point.line as f32 * cell_h;

            match icon.kind {
                NebulaPowerlineIconKind::Folder => {
                    Self::push_folder_icon(&mut quads, x, y, cell_w, cell_h, folder_color);
                },
                NebulaPowerlineIconKind::GitBranch => {
                    Self::push_git_branch_icon(&mut quads, x, y, cell_w, cell_h, branch_color);
                },
            }
        }

        self.renderer.draw_ui(&self.size_info, &quads);
    }

    fn push_folder_icon(
        quads: &mut Vec<UiQuad>,
        cell_x: f32,
        cell_y: f32,
        cell_w: f32,
        cell_h: f32,
        color: Rgb,
    ) {
        let icon_w = (cell_w * 1.18).clamp(8.0, cell_h * 0.72);
        let icon_h = (icon_w * 0.74).clamp(6.0, cell_h * 0.58);
        let x = cell_x + (cell_w - icon_w) * 0.5;
        let y = cell_y + (cell_h - icon_h) * 0.5 + cell_h * 0.02;
        let radius = (icon_h * 0.16).max(1.4);

        let glow = Self::rgba_from_rgb(color, 46);
        let main = Self::rgba_towards_white(color, 0.16, 236);
        let light = Self::rgba_towards_white(color, 0.34, 246);
        let shade = Self::rgba_towards_black(color, 0.16, 230);
        let shine = Rgba::new(255, 255, 255, 82);

        quads.push(UiQuad::glow(
            x - icon_w * 0.20,
            y - icon_h * 0.22,
            icon_w * 1.40,
            icon_h * 1.45,
            glow,
        ));
        quads.push(UiQuad::gradient(
            x + icon_w * 0.03,
            y + icon_h * 0.08,
            icon_w * 0.48,
            icon_h * 0.30,
            radius * 0.70,
            light,
            main,
            Gradient::Axis([0.9, 0.35]),
        ));
        quads.push(UiQuad::gradient(
            x,
            y + icon_h * 0.25,
            icon_w,
            icon_h * 0.68,
            radius,
            main,
            shade,
            Gradient::Axis([0.85, 0.45]),
        ));
        quads.push(UiQuad::solid(
            x + icon_w * 0.14,
            y + icon_h * 0.48,
            icon_w * 0.72,
            (cell_h * 0.035).max(1.0),
            0.8,
            shine,
        ));
    }

    fn push_git_branch_icon(
        quads: &mut Vec<UiQuad>,
        cell_x: f32,
        cell_y: f32,
        cell_w: f32,
        cell_h: f32,
        color: Rgb,
    ) {
        let icon = (cell_w * 1.12).clamp(7.0, cell_h * 0.68);
        let x = cell_x + (cell_w - icon) * 0.5;
        let y = cell_y + (cell_h - icon) * 0.5;
        let stroke = (icon * 0.13).clamp(1.15, 2.4);
        let node = (icon * 0.27).clamp(2.8, 5.0);
        let radius = node * 0.5;

        let main = Self::rgba_towards_white(color, 0.12, 240);
        let glow = Self::rgba_from_rgb(color, 42);
        let line = Self::rgba_towards_black(color, 0.08, 218);

        let trunk_x = x + icon * 0.34;
        let top_y = y + icon * 0.23;
        let mid_y = y + icon * 0.43;
        let bottom_y = y + icon * 0.78;
        let branch_x = x + icon * 0.70;

        quads.push(UiQuad::glow(x - icon * 0.20, y - icon * 0.18, icon * 1.42, icon * 1.40, glow));
        Self::push_icon_line(quads, trunk_x, top_y, trunk_x, bottom_y, stroke, line);
        Self::push_icon_line(quads, trunk_x, mid_y, branch_x, top_y, stroke, line);

        for (cx, cy) in [(trunk_x, top_y), (branch_x, top_y), (trunk_x, bottom_y)] {
            quads.push(UiQuad::solid(cx - node * 0.5, cy - node * 0.5, node, node, radius, main));
        }
    }

    fn push_icon_line(
        quads: &mut Vec<UiQuad>,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        width: f32,
        color: Rgba,
    ) {
        let dx = x1 - x0;
        let dy = y1 - y0;
        let len = (dx * dx + dy * dy).sqrt();
        if len <= f32::EPSILON {
            return;
        }

        let nx = -dy / len * width * 0.5;
        let ny = dx / len * width * 0.5;
        quads.push(UiQuad::poly(
            [[x0 + nx, y0 + ny], [x0 - nx, y0 - ny], [x1 + nx, y1 + ny], [x1 - nx, y1 - ny]],
            color,
            color,
            Gradient::None,
        ));
    }

    fn rgba_from_rgb(color: Rgb, alpha: u8) -> Rgba {
        Rgba::new(color.r, color.g, color.b, alpha)
    }

    fn rgba_towards_white(color: Rgb, amount: f32, alpha: u8) -> Rgba {
        Self::rgba_mix(color, Rgb::new(255, 255, 255), amount, alpha)
    }

    fn rgba_towards_black(color: Rgb, amount: f32, alpha: u8) -> Rgba {
        Self::rgba_mix(color, Rgb::new(0, 0, 0), amount, alpha)
    }

    fn rgba_mix(from: Rgb, to: Rgb, amount: f32, alpha: u8) -> Rgba {
        let t = amount.clamp(0.0, 1.0);
        let mix = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
        Rgba::new(mix(from.r, to.r), mix(from.g, to.g), mix(from.b, to.b), alpha)
    }
}