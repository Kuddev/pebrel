//! Terminal overlays: the search-regex formatter, the hyperlink preview, the
//! search / render-timer / line-indicator banners, damage highlighting, and
//! hint-highlight validation.

use unicode_width::UnicodeWidthChar;
use winit::window::CursorIcon;

use nebula_terminal::grid::Dimensions;
use nebula_terminal::index::{Column, Line, Point};
use nebula_terminal::term::cell::Flags;
use nebula_terminal::term::{self, LineDamageBounds};

use crate::config::UiConfig;
use crate::display::damage::damage_y_to_viewport_y;
use crate::renderer::rects::RenderRect;
use crate::renderer::ui::{Rgba, UiQuad};
use crate::string::{ShortenDirection, StrShortener};

use super::text_path_model::{fit_tail, strip_file_scheme};
use super::{DAMAGE_RECT_COLOR, Display, SHORTENER};

impl Display {
    /// Format search regex to account for the cursor and fullwidth characters.
    pub(super) fn format_search(
        search_regex: &str,
        search_label: &str,
        max_width: usize,
    ) -> String {
        let label_len = search_label.len();

        // Skip `search_regex` formatting if only label is visible.
        if label_len > max_width {
            return search_label[..max_width].to_owned();
        }

        // The search string consists of `search_label` + `search_regex` + `cursor`.
        let mut bar_text = String::from(search_label);
        bar_text.extend(StrShortener::new(
            search_regex,
            max_width.wrapping_sub(label_len + 1),
            ShortenDirection::Left,
            Some(SHORTENER),
        ));

        // Add place for cursor.
        bar_text.push(' ');

        bar_text
    }

    /// Draw preview for the currently highlighted `Hyperlink`.
    #[inline(never)]
    /// Draw a compact "open this link" tooltip near the hovered hint.
    ///
    /// 2026-07-23 用户反馈重构：上一版是整行 opaque `draw_string`，锚在鼠
    /// 标 cell 上——指针沿链接滑动时提示逐格跳动（被感知为"闪烁"），路
    /// 径还能占满整行（"显示太长"）。现在锚定到 hint 自己的起始 cell（指
    /// 针滑动时纹丝不动）、提示词压缩为 `Ctrl+点击`，并以 0.85× UI 锚定
    /// 字号画进圆角小气泡。
    ///
    /// 2026-07-26 用户反馈"显示不全"三连修：① file URI percent-decode 后
    /// 再展示（见 [`strip_file_scheme`]）；② 48 列硬帽退役，预算放开到整
    /// 个视口宽（fit_tail 仍兜底真溢出）；③ 气泡宽度按渲染器真实步进
    /// `average_advance × scale` 量取——`cell_w` 是 floor 后的值，48 列累
    /// 积下来尾部文字会戳出气泡右缘。
    pub(super) fn draw_hyperlink_preview(
        &mut self,
        config: &UiConfig,
        _cursor_point: Option<Point>,
        viewport_origin: Line,
    ) {
        let num_cols = self.size_info.columns();

        // The destination under the mouse (first highlighted hint with a URI)
        // plus that hint's start cell as the anchor.
        let Some((uri, hint_start)) =
            self.highlighted_hint.iter().chain(&self.vi_highlighted_hint).find_map(|hint| {
                hint.hyperlink().map(|h| (h.uri().to_owned(), *hint.bounds().start()))
            })
        else {
            return;
        };
        // Hint start scrolled out of the viewport → fall back to the mouse cell.
        let anchor = term::point_to_viewport_from(viewport_origin, hint_start).or_else(|| {
            self.hint_mouse_point.and_then(|p| term::point_to_viewport_from(viewport_origin, p))
        });
        let Some(anchor) = anchor else {
            return;
        };

        // Strip the `file://` scheme (and its leading slash before a Windows
        // drive) so a local path reads as a path, not a URL.
        let target = strip_file_scheme(&uri);
        const HINT: &str = " · Ctrl+点击";
        let width = |s: &str| -> usize { s.chars().map(|c| c.width().unwrap_or(0)).sum() };
        let hint_w = width(HINT);
        let target_budget = num_cols.saturating_sub(hint_w + 1);
        let target = fit_tail(&target, target_budget);
        let label = format!("{target}{HINT}");

        // Position: one row below the hint's first cell, or above on the last row.
        let line = if anchor.line + 1 < self.size_info.screen_lines() {
            anchor.line + 1
        } else {
            anchor.line.saturating_sub(1)
        };

        // Damage every row the bubble touches, this frame and next (it can
        // appear/vanish). The bubble is taller than one cell row (0.85·cell
        // plus padding and border, centered on its row), so it bleeds into
        // both neighbours — an un-damaged neighbour ghosts the bubble's edges
        // on partial-present paths.
        let last_line = self.size_info.screen_lines().saturating_sub(1);
        for touched in line.saturating_sub(1)..=(line + 1).min(last_line) {
            let damage = LineDamageBounds::new(touched, 0, num_cols);
            self.damage_tracker.frame().damage_line(damage);
            self.damage_tracker.next_frame().damage_line(damage);
        }

        let scale_px = self.window.scale_factor as f32;
        let s = |v: f32| v * scale_px;
        let text_scale = 0.85 * self.ui_text_scale();
        let cell_w = self.size_info.cell_width();
        let cell_h = self.size_info.cell_height();
        // Measure with the renderer's REAL step for scaled doc text — the
        // unfloored design advance (`draw_doc_text_tracked` walks
        // `average_advance × scale`). `cell_w` is that advance floored; the
        // fraction lost per column made long labels poke out of the bubble.
        let advance = self.glyph_cache.font_metrics().average_advance as f32 * text_scale;
        let label_px = width(&label) as f32 * advance;
        let pad_x = s(8.0);
        let bubble_w = label_px + 2.0 * pad_x;
        let bubble_h = cell_h * text_scale + s(8.0);
        let max_x = (self.size_info.width() - self.size_info.padding_right() - bubble_w).max(0.0);
        let x = (self.size_info.padding_x() + anchor.column.0 as f32 * cell_w).min(max_x);
        let y = self.size_info.padding_y() + line as f32 * cell_h + (cell_h - bubble_h) * 0.5;

        let fg = config.colors.footer_bar_foreground();
        let bg = config.colors.footer_bar_background();
        let quads = [
            UiQuad::solid(
                x - s(1.0),
                y - s(1.0),
                bubble_w + s(2.0),
                bubble_h + s(2.0),
                s(7.0),
                Rgba::new(fg.r, fg.g, fg.b, 46),
            ),
            UiQuad::solid(x, y, bubble_w, bubble_h, s(6.0), Rgba::new(bg.r, bg.g, bg.b, 240)),
        ];
        self.renderer.draw_ui(&self.size_info, &quads);

        let glyph_cache = &mut self.glyph_cache;
        let size = self.size_info;
        self.renderer.draw_doc_text_tracked(
            &size,
            x + pad_x,
            y + (bubble_h - cell_h * text_scale) * 0.5,
            text_scale,
            0.0,
            fg,
            Flags::empty(),
            &label,
            glyph_cache,
        );
    }

    /// Draw current search regex.
    #[inline(never)]
    pub(super) fn draw_search(&mut self, config: &UiConfig, text: &str) {
        // Assure text length is at least num_cols.
        let num_cols = self.size_info.columns();
        let text = format!("{text:<num_cols$}");

        let point = Point::new(self.size_info.screen_lines(), Column(0));

        let fg = config.colors.footer_bar_foreground();
        let bg = config.colors.footer_bar_background();

        self.renderer.draw_string(
            point,
            fg,
            bg,
            text.chars(),
            &self.size_info,
            &mut self.glyph_cache,
        );
    }

    /// Draw render timer.
    #[inline(never)]
    pub(super) fn draw_render_timer(&mut self, config: &UiConfig) {
        if !config.debug.render_timer {
            return;
        }

        let timing = format!("{:.3} usec", self.meter.average());
        let point = Point::new(self.size_info.screen_lines().saturating_sub(2), Column(0));
        let fg = config.colors.primary.background;
        let bg = config.colors.normal.red;

        // Damage render timer for current and next frame.
        let damage = LineDamageBounds::new(point.line, point.column.0, timing.len());
        self.damage_tracker.frame().damage_line(damage);
        self.damage_tracker.next_frame().damage_line(damage);

        let glyph_cache = &mut self.glyph_cache;
        self.renderer.draw_string(point, fg, bg, timing.chars(), &self.size_info, glyph_cache);
    }

    /// Draw an indicator for the position of a line in history.
    #[inline(never)]
    pub(super) fn draw_line_indicator(
        &mut self,
        config: &UiConfig,
        total_lines: usize,
        obstructed_column: Option<Column>,
        line: usize,
    ) {
        let columns = self.size_info.columns();
        let text = format!("[{}/{}]", line, total_lines - 1);
        let column = Column(self.size_info.columns().saturating_sub(text.len()));
        let point = Point::new(0, column);

        // Damage the line indicator for current and next frame.
        let damage = LineDamageBounds::new(point.line, point.column.0, columns - 1);
        self.damage_tracker.frame().damage_line(damage);
        self.damage_tracker.next_frame().damage_line(damage);

        let colors = &config.colors;
        let fg = colors.line_indicator.foreground.unwrap_or(colors.primary.background);
        let bg = colors.line_indicator.background.unwrap_or(colors.primary.foreground);

        // Do not render anything if it would obscure the vi mode cursor.
        if obstructed_column.is_none_or(|obstructed_column| obstructed_column < column) {
            let glyph_cache = &mut self.glyph_cache;
            self.renderer.draw_string(point, fg, bg, text.chars(), &self.size_info, glyph_cache);
        }
    }

    /// Highlight damaged rects.
    ///
    /// This function is for debug purposes only.
    pub(super) fn highlight_damage(&self, render_rects: &mut Vec<RenderRect>) {
        for damage_rect in &self.damage_tracker.shape_frame_damage(self.size_info.into()) {
            let x = damage_rect.x as f32;
            let height = damage_rect.height as f32;
            let width = damage_rect.width as f32;
            let y = damage_y_to_viewport_y(&self.size_info, damage_rect) as f32;
            let render_rect = RenderRect::new(x, y, width, height, DAMAGE_RECT_COLOR, 0.5);

            render_rects.push(render_rect);
        }
    }

    /// Check whether a hint highlight needs to be cleared.
    ///
    /// 2026-07-26 闪烁根因：这里原本拿共享 damage tracker 的 `intersects`
    /// 判断"hint 底下的网格变没变"，可 Nebula 每帧把 frame/next_frame 都标
    /// 成全窗 damage（全窗重绘呈现模型），`intersects` 的 `full ||` 短路恒
    /// 真——悬停高亮活不过两帧就被掐灭，鼠标一动重新点亮又立刻熄灭，ls
    /// 里扫过文件时下划线和气泡狂闪。改判终端自己上报的本帧 damage
    /// （`draw_pane` 在污染 tracker 之前捕获），语义回到上游本意。
    pub(super) fn validate_hint_highlights(
        &mut self,
        viewport_origin: Line,
        term_damage_full: bool,
        term_damage_lines: &[LineDamageBounds],
    ) {
        let hints = [
            (&mut self.highlighted_hint, &mut self.highlighted_hint_age, true),
            (&mut self.vi_highlighted_hint, &mut self.vi_highlighted_hint_age, false),
        ];

        let num_lines = self.size_info.screen_lines();
        for (hint, hint_age, reset_mouse) in hints {
            let (start, end) = match hint {
                Some(hint) => (*hint.bounds().start(), *hint.bounds().end()),
                None => continue,
            };

            // Ignore hints that were created this frame.
            *hint_age += 1;
            if *hint_age == 1 {
                continue;
            }

            // Convert hint bounds to viewport coordinates.
            let start = term::point_to_viewport_from(viewport_origin, start)
                .filter(|point| point.line < num_lines)
                .unwrap_or_default();
            let end = term::point_to_viewport_from(viewport_origin, end)
                .filter(|point| point.line < num_lines)
                .unwrap_or_else(|| Point::new(num_lines - 1, self.size_info.last_column()));

            // Clear hints whose underlying grid content actually changed.
            let grid_changed = term_damage_full
                || term_damage_lines.iter().any(|l| {
                    l.line >= start.line
                        && l.line <= end.line
                        // On the hint's first/last line only the hint's own
                        // column span counts; interior lines count wholly.
                        && (l.line != start.line || l.right >= start.column.0)
                        && (l.line != end.line || l.left <= end.column.0)
                });
            if grid_changed {
                if reset_mouse {
                    self.window.set_mouse_cursor(CursorIcon::Default);
                }
                self.damage_tracker.frame().mark_fully_damaged();
                *hint = None;
            }
        }
    }
}
