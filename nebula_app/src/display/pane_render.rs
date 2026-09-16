//! Pane rendering: `draw_pane`, the single-terminal paint routine that lays
//! out grid cells, cursor, overlays (search / IME / message bar), inline ghost
//! suggestions, math coverage and the scrollback scrollbar for one pane.

use std::num::NonZeroU32;

use parking_lot::MutexGuard;

use nebula_terminal::event::EventListener;
use nebula_terminal::grid::Dimensions;
use nebula_terminal::index::{Column, Direction, Point};
use nebula_terminal::term::cell::Flags;
use nebula_terminal::term::{self, Term, TermDamage, TermMode};
use nebula_terminal::vte::ansi::{CursorShape, NamedColor};

use crate::config::UiConfig;
use crate::display::content::{RenderableContent, RenderableCursor};
use crate::display::cursor::IntoRects;
use crate::display::hint::HintMatch;
use crate::event::SearchState;
use crate::message_bar::{self, MessageBuffer, MessageType};
use crate::renderer::rects::{RenderLines, RenderRect};

use super::hint;
use super::input_state::{nebula_input_from_raw_grid, nebula_raw_grid_row_preview};
use super::powerline_icons::{NebulaPowerlineIcon, NebulaPowerlineIconKind};
use super::terminal_math;
use super::{
    BACKWARD_SEARCH_LABEL, Display, FORWARD_SEARCH_LABEL, NEBULA_FOLDER_ICON_MARKER,
    NEBULA_GIT_BRANCH_ICON_MARKER, NebulaPaneState, SizeInfo, alt_screen_vertical_padding_bands,
    nebula_debug_log,
};

impl Display {
    /// Draw the screen.
    ///
    /// A reference to Term whose state is being drawn must be provided.
    ///
    /// This call may block if vsync is enabled.
    /// Render a single terminal into the region described by `view`.
    ///
    /// This paints grid cells, cursor, overlays (search/IME/message bar) and the
    /// inline ghost suggestion, but it does NOT clear (unless `clear_first`),
    /// draw the window chrome, or present — those are the caller's job so that
    /// multiple panes can share one frame. `force_focus` overrides the terminal's
    /// own focus state for split panes (`None` keeps the real window focus).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_pane<T: EventListener>(
        &mut self,
        mut terminal: MutexGuard<'_, Term<T>>,
        message_buffer: &MessageBuffer,
        config: &UiConfig,
        search_state: &mut SearchState,
        pane_state: &mut NebulaPaneState,
        view: SizeInfo,
        force_focus: Option<bool>,
        clear_first: bool,
    ) {
        // Override focus for split panes so the unfocused side shows a hollow
        // cursor; in single-pane mode keep the real window focus state.
        if let Some(focused) = force_focus {
            terminal.is_focused = focused;
        }

        // 把设置页的光标默认值同步进每一个被渲染的终端。事件路径只覆盖
        // "当前聚焦"的那一个 Term，新建 tab、分屏或后台 pane 都会漏掉；
        // set_default_cursor_style 内部有相等短路，逐帧调用无重绘代价。
        terminal.set_default_cursor_style(self.nebula_default_cursor_style());

        // Tell the renderer the full window height so pane viewports flip
        // correctly into OpenGL's bottom-left origin — matters for top/bottom
        // splits, where panes occupy different vertical bands of the window.
        self.renderer.set_window_height(self.size_info.height());

        // Collect renderable content before the terminal is dropped.
        let custom_background = self.nebula_background;
        let clickable_matches = hint::visible_clickable_matches(&terminal, config);
        let mut content = RenderableContent::new(config, self, &terminal, search_state, &view);
        let mut grid_cells = Vec::new();
        let mut grid_pad_bg = None;
        for cell in &mut content {
            if grid_pad_bg.is_none() && cell.bg_alpha > 0.0 {
                grid_pad_bg = Some(cell.bg);
            }
            grid_cells.push(cell);
        }
        let selection_range = content.selection_range();
        nebula_debug_log(format!(
            "render_pane clear_first={clear_first} view={}x{} pad=({:.0},{:.0},{:.0},{:.0}) selection={selection_range:?}",
            view.width(),
            view.height(),
            view.padding_x(),
            view.padding_right(),
            view.padding_y(),
            view.padding_bottom(),
        ));
        let foreground_color = content.color(NamedColor::Foreground as usize);
        let background_color =
            custom_background.unwrap_or_else(|| content.color(NamedColor::Background as usize));
        let display_offset = content.display_offset();
        let viewport_origin = content.viewport_origin();
        let cursor = content.cursor();

        let cursor_point = terminal.grid().cursor.point;
        // Anchors for OSC 1337 inline images (absolute-line bookkeeping).
        let grid_scrolled_out = terminal.grid().scrolled_out();
        let image_anchor = grid_scrolled_out + terminal.grid().history_size();
        // Ghost text is suppressed on the alt screen (vim/less/etc.).
        let alt_screen = terminal.mode().contains(TermMode::ALT_SCREEN);
        let total_lines = terminal.grid().total_lines();
        let metrics = self.glyph_cache.font_metrics();
        let size_info = view;

        let vi_mode = terminal.mode().contains(TermMode::VI);
        let vi_cursor_point = if vi_mode { Some(terminal.vi_mode_cursor.point) } else { None };
        #[cfg(windows)]
        let line_override = if alt_screen || vi_mode || search_state.regex().is_some() {
            None
        } else {
            nebula_input_from_raw_grid(
                &terminal,
                cursor_point,
                &pane_state.line_buf,
                &pane_state.suggest_env,
            )
        };
        #[cfg(windows)]
        let row_preview = if alt_screen || vi_mode || search_state.regex().is_some() {
            None
        } else {
            Some(nebula_raw_grid_row_preview(&terminal, cursor_point))
        };

        // 打字（含 IME 组词）不影响网格内容，扫描保持开启；一旦这里随
        // preedit 关断，中文输入的每次拼音组合都会让全部公式闪回原文。
        //
        // 扫描不按 AI CLI 进程名门控：WSL/SSH 中只能看到 wsl.exe/ssh.exe。
        // 四类标准定界符使用同一内容判定；Vi、搜索和选区仍由终端接管。
        let terminal_math_overlays =
            if !vi_mode && search_state.regex().is_none() && selection_range.is_none() {
                // 光标所在逻辑行是活动输入，扫描必须放过它，否则正在敲的
                // 命令会被当成公式替换掉。备用屏幕里同样要放过：编辑器
                // （vim/nvim 看 .tex/.md）的光标就压在你要改的那一行上，
                // 把它换成渲染图等于让人没法编辑自己的源码。
                let visible_cursor = term::point_to_viewport_from(viewport_origin, cursor_point)
                    .filter(|point| {
                        point.line < view.screen_lines() && point.column.0 < view.columns()
                    });
                terminal_math::scan_visible(
                    &mut pane_state.terminal_math,
                    &terminal,
                    &view,
                    &grid_cells,
                    alt_screen,
                    visible_cursor,
                    foreground_color,
                )
            } else {
                Vec::new()
            };
        let math_pixel_size = self.glyph_cache.font_size.as_px();
        let math_pixels_per_point = crate::math::pixels_per_point(self.window.scale_factor as f32);
        let prepared_math = terminal_math::prepare_overlays(
            &mut pane_state.terminal_math,
            &terminal_math_overlays,
            &view,
            math_pixel_size,
            math_pixels_per_point,
        );
        let math_coverage =
            terminal_math::CoverageMask::build(&terminal_math_overlays, &prepared_math);
        // A normal shell line may reflow its suffix around a compact formula.
        // Full-screen TUIs own fixed grid geometry (sidebars, cards, status
        // bands), so moving every cell after an inline formula would also move
        // those ANSI backgrounds and tear the interface into coloured blocks.
        pane_state.terminal_math.update_projection(
            &terminal_math_overlays,
            &prepared_math,
            !alt_screen,
        );

        // Add damage from the terminal, keeping a pane-local copy: the shared
        // tracker gets flooded with a full-window mark every frame further
        // down, so "did the grid actually change?" (hint invalidation) must
        // be judged from the terminal's own report captured here.
        let mut term_damage_full = false;
        let mut term_damage_lines = Vec::new();
        match terminal.damage() {
            TermDamage::Full => {
                term_damage_full = true;
                self.damage_tracker.frame().mark_fully_damaged();
            },
            TermDamage::Partial(damaged_lines) => {
                for damage in damaged_lines {
                    self.damage_tracker.frame().damage_line(damage);
                    term_damage_lines.push(damage);
                }
            },
        }
        terminal.reset_damage();

        // Drop terminal as early as possible to free lock.
        drop(terminal);

        // Invalidate highlighted hints if grid has changed. Only the pane
        // that owns the hover may judge that: `highlighted_hint` is hit-tested
        // against the focused pane, so a background pane's output (build log,
        // `top`) must not tear down the foreground's highlight.
        if force_focus != Some(false) {
            self.validate_hint_highlights(viewport_origin, term_damage_full, &term_damage_lines);
        }

        // OSC 1337 inline images: prune rows that scrolled out of history for
        // good, then collect the ones visible in this pane's viewport for the
        // single full-window draw pass in `present_frame`.
        if !pane_state.inline_images.is_empty() {
            let cell_h = view.cell_height();
            pane_state.inline_images.retain(|img| {
                let rows = (img.height / cell_h).ceil().max(1.0) as usize;
                img.abs_line + rows >= grid_scrolled_out
            });
            let top_abs = (image_anchor as i64 + viewport_origin.0 as i64) as f32;
            for img in &pane_state.inline_images {
                let y = view.padding_y() + (img.abs_line as f32 - top_abs) * cell_h;
                // Cull images entirely outside this pane's band.
                if y + img.height <= view.padding_y() - cell_h
                    || y >= view.padding_y() + view.height()
                {
                    continue;
                }
                self.nebula_frame_images.push((
                    img.id,
                    img.rgba.clone(),
                    (img.px_w, img.px_h),
                    (view.padding_x(), y, img.width, img.height),
                ));
            }
        }

        // Refresh the inline ghost-text suggestion. On Windows the input is read
        // off the grid (screen truth, never desyncs); elsewhere the tracked
        // `line_buf` is used. Only on the primary screen, never during vi/search
        // overlays.
        if alt_screen || vi_mode || search_state.regex().is_some() {
            pane_state.clear_completion_hints();
        } else {
            #[cfg(windows)]
            {
                // No prompt arrow before the cursor (or a mid-line edit) means we
                // cannot trust a hint here — clear it rather than guess.
                if !pane_state.line_buf.is_empty()
                    || line_override.as_ref().is_some_and(|s| !s.is_empty())
                {
                    nebula_debug_log(format!(
                        "grid_input cwd={:?} line_buf={:?} raw={:?} cursor=line:{} col:{} row={:?}",
                        pane_state.cwd,
                        pane_state.line_buf,
                        line_override,
                        cursor_point.line.0,
                        cursor_point.column.0,
                        row_preview
                    ));
                }
                match line_override {
                    Some(line) => {
                        pane_state.screen_line = line.clone();
                        self.nebula_update_suggestion(pane_state, Some(line));
                    },
                    None => {
                        pane_state.screen_line.clear();
                        pane_state.clear_completion_hints();
                    },
                }
            }
            #[cfg(not(windows))]
            self.nebula_update_suggestion(pane_state, None);
        }

        // Add damage from nebula's UI elements overlapping terminal.

        // Nebula always redraws and presents the full window: the chrome
        // (clock, ambient glow, gradient border) is painted every frame, and
        // partial damage would leave terminal content (prompt, scrollback)
        // stale after the window is occluded or sent to the background.
        let _ = (self.visual_bell.intensity(), self.hint_state.active(), search_state.regex());
        self.damage_tracker.frame().mark_fully_damaged();
        self.damage_tracker.next_frame().mark_fully_damaged();

        let vi_cursor_viewport_point = vi_cursor_point.and_then(|cursor| {
            term::point_to_viewport_from(viewport_origin, cursor).filter(|point| {
                point.line < size_info.screen_lines() && point.column.0 < size_info.columns()
            })
        });
        self.damage_tracker.damage_vi_cursor(vi_cursor_viewport_point);
        self.damage_tracker.damage_selection(selection_range, display_offset);

        // Make sure this window's OpenGL context is active. The caller is
        // expected to have already activated it; calling again is cheap and
        // keeps `draw_pane` safe to invoke standalone.
        self.make_current();

        // Only the first pane of a frame clears the whole window; subsequent
        // panes paint on top of the shared, already-cleared backdrop.
        if clear_first {
            // Layer model: the window clears to the opaque shell color (the
            // chrome backdrop), then the terminal is painted as a rounded
            // `term_bg` card floating on it. Default-background cells draw no
            // background of their own (bg_alpha == 0), so they show the card.
            nebula_debug_log(format!(
                "render_clear path=pane window={}x{} alpha={:.3}",
                self.size_info.width(),
                self.size_info.height(),
                self.nebula_window_opacity,
            ));
            self.draw_window_backdrop(background_color);
        }

        // 分屏渲染时每个 pane 都有独立的 viewport/projection；否则右侧内容会沿用上一帧
        // 或左侧 pane 的坐标系，最终叠到左边而不是显示在右边。
        self.renderer.resize(&size_info);

        let mut lines = RenderLines::new();

        // Optimize loop hint comparator.
        let has_highlighted_hint =
            self.highlighted_hint.is_some() || self.vi_highlighted_hint.is_some();

        // Draw grid.
        let mut powerline_icons = Vec::new();
        {
            let _sampler = self.meter.sampler();

            // Ensure macOS hasn't reset our viewport.
            #[cfg(target_os = "macos")]
            self.renderer.set_viewport(&size_info);

            let glyph_cache = &mut self.glyph_cache;
            let highlighted_hint = &self.highlighted_hint;
            let vi_highlighted_hint = &self.vi_highlighted_hint;
            let damage_tracker = &mut self.damage_tracker;
            let mut clickable_index = 0usize;

            let cells = grid_cells.into_iter().filter_map(|mut cell| {
                let source_point = cell.point;
                // Hide formula source glyphs while retaining each terminal
                // cell's resolved background.
                let formula_source =
                    !math_coverage.is_empty() && math_coverage.covers(source_point);
                if formula_source {
                    cell.character = ' ';
                    cell.flags.remove(Flags::ALL_UNDERLINES | Flags::STRIKEOUT);
                    cell.extra = None;
                }
                // 这里只改 RenderableCell 副本的屏幕列，terminal grid 中的
                // 源列始终不动；宽字符、背景和装饰随后都会读取同一个 point。
                cell.point = if formula_source {
                    pane_state
                        .terminal_math
                        .project_formula_background(source_point, size_info.columns())?
                } else {
                    pane_state.terminal_math.project_cell(source_point, size_info.columns())?
                };
                match cell.character {
                    NEBULA_FOLDER_ICON_MARKER => {
                        powerline_icons.push(NebulaPowerlineIcon {
                            kind: NebulaPowerlineIconKind::Folder,
                            point: cell.point,
                        });
                        cell.character = ' ';
                    },
                    NEBULA_GIT_BRANCH_ICON_MARKER => {
                        powerline_icons.push(NebulaPowerlineIcon {
                            kind: NebulaPowerlineIconKind::GitBranch,
                            point: cell.point,
                        });
                        cell.character = ' ';
                    },
                    _ => (),
                }

                let point = term::viewport_to_point_from(viewport_origin, source_point);
                while clickable_matches
                    .get(clickable_index)
                    .is_some_and(|bounds| bounds.end() < &point)
                {
                    clickable_index += 1;
                }
                let is_clickable = clickable_matches
                    .get(clickable_index)
                    .is_some_and(|bounds| bounds.contains(&point));
                if is_clickable {
                    // 点击目标的虚线直接继承每个 cell 的文字色；不能统一成主题色，
                    // 否则 ls 的目录/可执行文件颜色语义会被下划线悄悄抹平。
                    cell.flags.remove(Flags::ALL_UNDERLINES);
                    cell.flags.insert(Flags::DASHED_UNDERLINE);
                    cell.underline = cell.fg;
                }

                // Underline hints hovered by mouse or vi mode cursor. Persistent
                // clickable ranges stay dashed; other hint states retain the
                // stronger solid underline used by keyboard/vi highlighting.
                if has_highlighted_hint {
                    let hyperlink = cell.extra.as_ref().and_then(|extra| extra.hyperlink.as_ref());

                    let should_highlight = |hint: &Option<HintMatch>| {
                        hint.as_ref().is_some_and(|hint| hint.should_highlight(point, hyperlink))
                    };
                    if should_highlight(highlighted_hint) || should_highlight(vi_highlighted_hint) {
                        damage_tracker.frame().damage_point(source_point);
                        if !is_clickable {
                            cell.flags.insert(Flags::UNDERLINE);
                        }
                    }
                }

                // Update underline/strikeout.
                lines.update(&cell);

                Some(cell)
            });
            self.renderer.draw_cells(&size_info, glyph_cache, cells);
        }

        let mut rects = lines.rects(&metrics, &size_info);

        if alt_screen {
            if let Some(pad_bg) = grid_pad_bg {
                let (_, card_y, _, card_h) = self.terminal_card_rect();
                let x = size_info.padding_x();
                let w = size_info.width() - size_info.padding_x() - size_info.padding_right();
                // 备用屏幕会给整张网格着色。补齐背景时只能填当前 Pane 的边缘；
                // 下方 Pane 若从整张卡片顶部开始填，会在最后绘制时盖住上方 Pane。
                for (y, height) in
                    alt_screen_vertical_padding_bands(&self.size_info, &size_info, card_y, card_h)
                        .into_iter()
                        .flatten()
                {
                    rects.push(RenderRect::new(x, y, w, height, pad_bg, 1.0));
                }
            }
        }

        if let Some(vi_cursor_point) = vi_cursor_point {
            // Indicate vi mode by showing the cursor's position in the top right corner.
            let line = (-vi_cursor_point.line.0 + size_info.bottommost_line().0) as usize;
            let obstructed_column = Some(vi_cursor_point)
                .filter(|point| point.line == -(display_offset as i32))
                .map(|point| point.column);
            self.draw_line_indicator(config, total_lines, obstructed_column, line);
        } else if search_state.regex().is_some() {
            // Show current display offset in vi-less search to indicate match position.
            self.draw_line_indicator(config, total_lines, None, display_offset);
        };

        // Draw cursor.
        rects.extend(cursor.rects(&size_info, config.cursor.thickness()));

        // Push visual bell after url/underline/strikeout rects.
        let visual_bell_intensity = self.visual_bell.intensity();
        if visual_bell_intensity != 0. {
            let visual_bell_rect = RenderRect::new(
                0.,
                0.,
                size_info.width(),
                size_info.height(),
                config.bell.color,
                visual_bell_intensity as f32,
            );
            rects.push(visual_bell_rect);
        }

        // Handle IME positioning and search bar rendering.
        let ime_position = match search_state.regex() {
            Some(regex) => {
                let search_label = match search_state.direction() {
                    Direction::Right => FORWARD_SEARCH_LABEL,
                    Direction::Left => BACKWARD_SEARCH_LABEL,
                };

                let search_text = Self::format_search(regex, search_label, size_info.columns());

                // Render the search bar.
                self.draw_search(config, &search_text);

                // Draw search bar cursor.
                let line = size_info.screen_lines();
                let column = Column(search_text.chars().count() - 1);

                // Add cursor to search bar if IME is not active.
                if self.ime.preedit().is_none() {
                    let fg = config.colors.footer_bar_foreground();
                    let shape = CursorShape::Underline;
                    let cursor_width = NonZeroU32::new(1).unwrap();
                    let cursor =
                        RenderableCursor::new(Point::new(line, column), shape, fg, cursor_width);
                    rects.extend(cursor.rects(&size_info, config.cursor.thickness()));
                }

                Some(Point::new(line, column))
            },
            None => {
                let num_lines = size_info.screen_lines();
                match vi_cursor_viewport_point {
                    None => term::point_to_viewport_from(viewport_origin, cursor_point).filter(
                        |point| point.line < num_lines && point.column.0 < size_info.columns(),
                    ),
                    point => point,
                }
            },
        };

        // Handle IME.
        if self.ime.is_enabled() {
            if let Some(point) = ime_position {
                let (fg, bg) = if search_state.regex().is_some() {
                    (config.colors.footer_bar_foreground(), config.colors.footer_bar_background())
                } else {
                    (foreground_color, background_color)
                };

                self.draw_ime_preview(point, fg, bg, &mut rects, config);
            }
        }

        if let Some(message) = message_buffer.message() {
            let search_offset = usize::from(search_state.regex().is_some());
            let text = message.text(&size_info);

            // Create a new rectangle for the background.
            let start_line = size_info.screen_lines() + search_offset;
            let bar = message_bar::message_bar_rect(&size_info, search_offset != 0);

            let bg = match message.ty() {
                MessageType::Error => config.colors.normal.red,
                MessageType::Warning => config.colors.normal.yellow,
            };

            let x = bar.x as i32;
            let y = bar.y as i32;
            let width = bar.width as i32;
            let height = bar.height as i32;
            let message_bar_rect = RenderRect::new(bar.x, bar.y, bar.width, bar.height, bg, 1.);

            // Push message_bar in the end, so it'll be above all other content.
            rects.push(message_bar_rect);

            // Always damage message bar, since it could have messages of the same size in it.
            self.damage_tracker.frame().add_viewport_rect(&size_info, x, y, width, height);

            // Draw rectangles.
            self.renderer.draw_rects(&size_info, &metrics, rects);

            // Relay messages to the user.
            let glyph_cache = &mut self.glyph_cache;
            let fg = config.colors.primary.background;
            for (i, message_text) in text.iter().enumerate() {
                let point = Point::new(start_line + i, Column(0));
                self.renderer.draw_string(
                    point,
                    fg,
                    bg,
                    message_text.chars(),
                    &size_info,
                    glyph_cache,
                );
            }

            // 关闭按钮交给 chrome pass 自绘：这里是终端文字管线，画不了圆角
            // 底和图标墨迹。发布几何 + 墨色，`draw_message_close` 随后照着画，
            // 与 `message_close_button_rect` 共用同一份矩形。
            self.nebula_message_close =
                message_bar::message_close_button_rect(&size_info, search_offset != 0)
                    .map(|rect| ((rect.x, rect.y, rect.width, rect.height), fg));
        } else {
            self.nebula_message_close = None;
            self.nebula_message_close_hover = false;
            // Draw rectangles.
            self.renderer.draw_rects(&size_info, &metrics, rects);
        }

        terminal_math::draw_overlays(
            &mut self.renderer,
            &mut self.glyph_cache,
            &mut pane_state.terminal_math,
            &terminal_math_overlays,
            &prepared_math,
            &size_info,
            math_pixels_per_point,
        );

        self.draw_powerline_icons(&powerline_icons, size_info);
        // `draw_powerline_icons` uses the full-window UI renderer and restores
        // a full-window viewport; bind the pane projection again before drawing
        // the inline ghost suggestion.
        self.renderer.resize(&size_info);

        // Draw inline ghost-text autosuggestion directly after the cursor,
        // once everything else for the cell row is on screen. The color is
        // the theme's faintest ink (not a fixed gray), so on light themes it
        // stays clearly weaker than the near-black real input instead of
        // colliding with it.
        if !pane_state.suggestion.is_empty() && self.ime.preedit().is_none() {
            if let Some(point) = term::point_to_viewport_from(viewport_origin, cursor_point)
                .filter(|p| p.line < size_info.screen_lines() && p.column.0 < size_info.columns())
            {
                let avail = size_info.columns() - point.column.0;
                let ghost: String = pane_state.suggestion.chars().take(avail).collect();
                let ghost_fg = self.nebula_theme.skin().ink_faint;
                let glyph_cache = &mut self.glyph_cache;
                self.renderer.draw_string(
                    point,
                    ghost_fg,
                    background_color,
                    ghost.chars(),
                    &size_info,
                    glyph_cache,
                );
            }
        }

        // Popup-style completion list (弹窗补齐): candidate rows anchored to
        // the prompt cursor, keyboard-selected. Mutually exclusive with the
        // ghost above by construction; same IME suppression.
        if !pane_state.completion_items.is_empty() && self.ime.preedit().is_none() {
            if let Some(anchor) = term::point_to_viewport_from(viewport_origin, cursor_point)
                .filter(|p| p.line < size_info.screen_lines() && p.column.0 < size_info.columns())
            {
                self.draw_completion_popup(
                    &pane_state.completion_items,
                    pane_state.completion_selected,
                    anchor,
                    &size_info,
                    background_color,
                );
            }
        }

        self.draw_render_timer(config);

        // Draw hyperlink uri preview.
        if has_highlighted_hint {
            let cursor_point = vi_cursor_point.or(Some(cursor_point));
            self.draw_hyperlink_preview(config, cursor_point, viewport_origin);
        }

        // Overlay scrollbar on the right edge while scrolled into history.
        self.draw_scrollbar(&size_info, display_offset, total_lines);
    }
}
