//! Frame pipeline: the update/event pump (`handle_update`,
//! `process_renderer_update`) and the per-frame draw orchestration -- the
//! full-window `draw` entry, the pane/doc/image/settings frame wrappers,
//! split overlays, and the scrollback scrollbar geometry and hit-testing.

use std::mem;
use std::num::NonZeroU32;

use glutin::prelude::*;
use log::info;
use parking_lot::MutexGuard;
use winit::dpi::PhysicalSize;

use nebula_terminal::event::{EventListener, OnResize};
use nebula_terminal::grid::Dimensions as TermDimensions;
use nebula_terminal::term::Term;
use nebula_terminal::vte::ansi::NamedColor;

use super::ux_anims::ResizeHud;
use super::{
    Display, NEBULA_UNFOCUSED_SPLIT_DIM, NebulaPaneState, SizeInfo, SplitDirection,
    bottom_content_reserve, chrome_reserve, content_pad_x, nebula_debug_log, nebula_link_log,
    sidebar_width,
};
use super::{image_viewer, markdown_view};

use crate::config::UiConfig;
use crate::display::color::Rgb;
use crate::event::SearchState;
use crate::message_bar::MessageBuffer;
use crate::renderer::ui::{Rgba, UiQuad};
use crate::scheduler::Scheduler;

impl Display {
    // XXX: this function must not call to any `OpenGL` related tasks. Renderer updates are
    // performed in [`Self::process_renderer_update`] right before drawing.
    //
    /// Process update events.
    pub fn handle_update<T>(
        &mut self,
        // Grid resizes are committed together with the PTY by the window
        // context (leading edge / settle); the handle stays in the signature
        // so the call sites don't churn if an immediate path returns.
        _terminal: &mut Term<T>,
        // PTY resizes are deferred to the settle timer (see
        // `nebula_pty_resize_pending`); the handle stays in the signature so
        // the call sites don't churn if an immediate path returns.
        _pty_resize_handle: &mut dyn OnResize,
        message_buffer: &MessageBuffer,
        search_state: &mut SearchState,
        config: &UiConfig,
    ) where
        T: EventListener,
    {
        let pending_update = mem::take(&mut self.pending_update);

        let (mut cell_width, mut cell_height) =
            (self.size_info.cell_width(), self.size_info.cell_height());

        if pending_update.font().is_some() || pending_update.cursor_dirty() {
            let renderer_update = self.pending_renderer_update.get_or_insert(Default::default());
            renderer_update.clear_font_cache = true
        }

        // Update font size and cell dimensions.
        if let Some(font) = pending_update.font() {
            let cell_width_mode = self.nebula_cell_width_mode;
            let cell_dimensions =
                Self::update_font_size(&mut self.glyph_cache, config, font, cell_width_mode);
            cell_width = cell_dimensions.0;
            cell_height = cell_dimensions.1;

            info!("Cell size: {cell_width} x {cell_height}");

            // The window floor is derived from the cell size, so it has to be
            // re-derived here or zooming in would leave the old (smaller) floor
            // in place and reopen the narrow-collapse hole.
            #[cfg(windows)]
            self.apply_min_window_size(config, cell_width, cell_height);

            // Every zoom / font / DPI change funnels through a font update,
            // so this is the single point where the UI font role can go
            // stale.
            self.refresh_ui_font(config);

            // Mark entire terminal as damaged since glyph size could change without cell size
            // changes.
            self.damage_tracker.frame().mark_fully_damaged();
        }

        let (mut width, mut height) = (self.size_info.width(), self.size_info.height());
        if let Some(dimensions) = pending_update.dimensions() {
            width = dimensions.width as f32;
            height = dimensions.height as f32;
        }

        let padding = config.window.padding(self.window.scale_factor as f32);
        let chrome = chrome_reserve(self.window.scale_factor as f32);

        let scale = self.window.scale_factor as f32;
        let content_pad = content_pad_x(scale);
        let sidebar = sidebar_width(scale, self.nebula_sidebar_collapsed, self.nebula_sidebar_w);
        // The file/git drawer occupies real layout space: the grid cedes its
        // width (plus the window margin) on the right, exactly like the left
        // sidebar reserve — it does not float over the terminal.
        let drawer = if self.nebula_side_panel.open {
            ((self.nebula_drawer_w * scale).min(width * 0.42) + 8.0 * scale).round()
        } else {
            0.0
        };
        let mut new_size = SizeInfo::new_fully_asymmetric(
            width,
            height,
            cell_width,
            cell_height,
            padding.0 + content_pad + sidebar,
            padding.0 + content_pad + drawer,
            padding.1 + chrome,
            padding.1 + bottom_content_reserve(scale),
        );

        // Update number of column/lines in the viewport.
        let search_active = search_state.history_index.is_some();
        let message_bar_lines = message_buffer.message().map_or(0, |m| m.text(&new_size).len());
        let search_lines = usize::from(search_active);
        new_size.reserve_lines(message_bar_lines + search_lines);

        // Update resize increments.
        if config.window.resize_increments {
            let increments = self
                .window
                .allows_drag_resize()
                .then_some(PhysicalSize::new(cell_width, cell_height));
            self.window.set_resize_increments(increments);
        }

        // Update the visible terminal viewport when its dimensions have changed.
        if self.size_info.screen_lines() != new_size.screen_lines
            || self.size_info.columns() != new_size.columns()
        {
            // Defer the PTY resize to the settle timer instead of notifying
            // per tick: the in-box ConPTY repaints its entire viewport on
            // every resize, so drag-resizing would flood the scrollback with
            // dozens of shredded repaints (and TUIs like Claude Code redraw
            // storms).  The window context commits the grid and ConPTY
            // together at the leading/trailing edges; until then rendering is
            // clipped to the last committed grid, so both sides retain the
            // same reflow history.
            self.nebula_pty_resize_pending = true;

            // Resize damage tracking.
            self.damage_tracker.resize(new_size.screen_lines(), new_size.columns());

            // Flash a transient "cols × rows" HUD, skipping the first (startup)
            // resize so nothing flashes when the window is first created.
            if self.nebula_resize_hud_armed {
                self.nebula_resize_hud =
                    Some(ResizeHud::new(new_size.columns(), new_size.screen_lines()));
            }
            self.nebula_resize_hud_armed = true;
            nebula_link_log(format!(
                "viewport_resize {}x{} px={width}x{height} pad_x={} pad_r={} pad_y={} \
                 cell={cell_width}x{cell_height} drawer={drawer} sidebar={sidebar} \
                 reserved={}",
                new_size.columns(),
                new_size.screen_lines(),
                new_size.padding_x(),
                new_size.padding_right(),
                new_size.padding_y(),
                message_bar_lines + search_lines,
            ));
        }

        // Check if dimensions have changed.
        if new_size != self.size_info {
            // Queue renderer update.
            let renderer_update = self.pending_renderer_update.get_or_insert(Default::default());
            renderer_update.resize = true;

            // Clear focused search match.
            search_state.clear_focused_match();
        }
        self.size_info = new_size;
    }

    // NOTE: Renderer updates are split off, since platforms like Wayland require resize and other
    // OpenGL operations to be performed right before rendering. Otherwise they could lock the
    // back buffer and render with the previous state. This also solves flickering during resizes.
    //
    /// Update the state of the renderer.
    pub fn process_renderer_update(&mut self) {
        let renderer_update = match self.pending_renderer_update.take() {
            Some(renderer_update) => renderer_update,
            _ => return,
        };

        // Resize renderer.
        if renderer_update.resize {
            let width = NonZeroU32::new(self.size_info.width() as u32).unwrap();
            let height = NonZeroU32::new(self.size_info.height() as u32).unwrap();
            self.surface.resize(&self.context, width, height);
        }

        // Ensure we're modifying the correct OpenGL context.
        self.make_current();

        if renderer_update.clear_font_cache {
            self.reset_glyph_cache();
        }

        self.renderer.resize(&self.size_info);

        info!("Padding: {} x {}", self.size_info.padding_x(), self.size_info.padding_y());
        info!("Width: {}, Height: {}", self.size_info.width(), self.size_info.height());
    }

    /// Draw the screen for a single, full-window terminal.
    ///
    /// A reference to the Term whose state is being drawn must be provided.
    /// This call may block if vsync is enabled.
    pub fn draw<T: EventListener>(
        &mut self,
        terminal: MutexGuard<'_, Term<T>>,
        scheduler: &mut Scheduler,
        message_buffer: &MessageBuffer,
        config: &UiConfig,
        search_state: &mut SearchState,
        pane_state: &mut NebulaPaneState,
    ) {
        let view = self.size_info;
        self.make_current();
        self.reload_nebula_settings_if_changed(config);
        // 光标聚焦态以 winit 的窗口焦点为唯一权威：`Term::is_focused` 是由
        // Focused 事件维护的缓存，只写"当时聚焦"的那一个 Term——切 tab /
        // 分屏又并回后残留旧值，表现为聚焦窗口里光标随机空心、不闪。每帧
        // 用真实焦点覆盖，残留状态无处藏身。
        self.draw_pane(
            terminal,
            message_buffer,
            config,
            search_state,
            pane_state,
            view,
            Some(self.window.has_focus()),
            true,
        );
        self.present_frame(scheduler);
    }

    /// Begin a multi-pane frame: bind the GL context and refresh themed
    /// settings before the per-pane draws.
    pub fn begin_pane_frame(&mut self, config: &UiConfig) {
        self.reload_nebula_settings_if_changed(config);
        self.make_current();
    }

    /// Draw a document-viewer tab's frame: the shell backdrop and terminal
    /// card exactly like a pane frame (same layer model), then the document
    /// instead of a grid, then the normal chrome via `present_frame`.
    pub fn draw_doc_frame(
        &mut self,
        doc: &mut markdown_view::DocView,
        _view: SizeInfo,
        scheduler: &mut Scheduler,
    ) {
        self.renderer.set_window_height(self.size_info.height());

        nebula_debug_log(format!(
            "render_clear path=document window={}x{} alpha={:.3}",
            self.size_info.width(),
            self.size_info.height(),
            self.nebula_window_opacity,
        ));
        let card_bg = self.nebula_background.unwrap_or(self.colors[NamedColor::Background]);
        self.draw_window_backdrop(card_bg);
        let scale = self.window.scale_factor as f32;
        let area = self.doc_view_area();
        let skin = self.nebula_theme.skin();
        let size = self.size_info;
        markdown_view::draw(
            doc,
            &mut self.renderer,
            &mut self.glyph_cache,
            &size,
            &skin,
            area,
            scale,
            doc.scrollbar_hover(),
        );

        self.present_frame(scheduler);
    }

    pub fn draw_image_frame(
        &mut self,
        image: &image_viewer::ImageView,
        _view: SizeInfo,
        scheduler: &mut Scheduler,
    ) {
        self.renderer.set_window_height(self.size_info.height());
        let card_bg = self.nebula_background.unwrap_or(self.colors[NamedColor::Background]);
        self.draw_window_backdrop(card_bg);
        let scale = self.window.scale_factor as f32;
        let area = self.image_view_area();
        image.draw(&mut self.renderer, &self.size_info, area, scale);
        self.present_frame(scheduler);
    }

    /// Draw the Settings special tab. Its controls are emitted by the chrome
    /// pass so they retain the same hit geometry and icon texture pipeline as
    /// the rest of Nebula, but the base is a normal tab content card.
    pub fn draw_settings_frame(&mut self, scheduler: &mut Scheduler) {
        self.renderer.set_window_height(self.size_info.height());

        nebula_debug_log(format!(
            "render_clear path=settings window={}x{} alpha={:.3}",
            self.size_info.width(),
            self.size_info.height(),
            self.nebula_window_opacity,
        ));
        let card_bg = self.nebula_background.unwrap_or(self.colors[NamedColor::Background]);
        self.draw_window_backdrop(card_bg);

        self.present_frame(scheduler);
    }

    /// Draw one pane of a multi-pane layout into `view`. `clear_first` clears
    /// the whole window before the first pane; later panes paint on top.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_pane_view<T: EventListener>(
        &mut self,
        terminal: MutexGuard<'_, Term<T>>,
        message_buffer: &MessageBuffer,
        config: &UiConfig,
        search_state: &mut SearchState,
        pane_state: &mut NebulaPaneState,
        view: SizeInfo,
        focused: bool,
        clear_first: bool,
    ) {
        self.draw_pane(
            terminal,
            message_buffer,
            config,
            search_state,
            pane_state,
            view,
            Some(focused),
            clear_first,
        );
    }

    /// Overlay split chrome over the drawn panes: dim every unfocused pane and
    /// paint divider hairlines. Rectangles are screen-space `(x, y, w, h)` with
    /// a top-left origin. Focus reads as a brightness difference
    /// `unfocused-split-opacity`) rather than an outline.
    pub fn draw_split_overlays(
        &mut self,
        dim_rects: &[(f32, f32, f32, f32)],
        divider_rects: &[(f32, f32, f32, f32)],
    ) {
        let palette = self.nebula_theme.palette();
        let veil = Rgba::new(0, 0, 0, 0).with_alpha(NEBULA_UNFOCUSED_SPLIT_DIM);
        let line_color = palette.edge_l.with_alpha(0.35);

        let mut quads: Vec<UiQuad> = Vec::with_capacity(dim_rects.len() + divider_rects.len() + 1);
        for &(x, y, w, h) in dim_rects {
            if w > 0.0 && h > 0.0 {
                quads.push(UiQuad::solid(x, y, w, h, 0.0, veil));
            }
        }
        for &(x, y, w, h) in divider_rects {
            if w > 0.0 && h > 0.0 {
                quads.push(UiQuad::solid(x, y, w, h, 0.0, line_color));
            }
        }

        // Freshly split pane slides in: a bg-coloured cover anchored at the
        // pane's far edge shrinks away over ~160ms (ease-out), so the new pane
        // wipes in from the divider instead of popping. Timestamp-derived, no
        // per-frame allocation (same discipline as the quick-terminal slide).
        if let Some(mut reveal) = self.nebula_split_reveal {
            reveal.motion.step(self.nebula_ui_anims.frame());
            let e = reveal.motion.value();
            if !reveal.motion.is_active() {
                self.nebula_split_reveal = None;
            } else {
                self.nebula_split_reveal = Some(reveal);
                let (x, y, w, h) = reveal.rect;
                let bg = self.nebula_background.unwrap_or(Rgb::new(15, 17, 26));
                let cover = Rgba::new(bg.r, bg.g, bg.b, 255);
                let (cx, cy, cw, chh) = match reveal.direction {
                    SplitDirection::LeftRight => (x + w * e, y, w * (1.0 - e), h),
                    SplitDirection::TopBottom => (x, y + h * e, w, h * (1.0 - e)),
                };
                if cw > 0.5 && chh > 0.5 {
                    quads.push(UiQuad::solid(cx, cy, cw, chh, 0.0, cover));
                }
                self.window.request_redraw();
            }
        }

        self.renderer.draw_ui(&self.size_info, &quads);
    }

    /// Finish a multi-pane frame: draw window chrome and present.
    pub fn finish_pane_frame(&mut self, scheduler: &mut Scheduler) {
        self.present_frame(scheduler);
    }

    /// Paint the divider between two split panes and dim the unfocused one.
    /// (Removed: superseded by `draw_split_overlays` + the layout tree in
    /// `window_context/split.rs`.)
    #[cfg(any())]
    fn _removed_split_helpers() {}

    /// Overlay scrollbar on the right edge of a pane, shown only while scrolled
    /// up into the scrollback (auto-hides at the bottom).
    /// overlay-style `scrollbar`: a thin, semi-transparent thumb floating over
    /// the grid's right edge, sized to the visible fraction of total content.
    pub(super) fn draw_scrollbar(
        &mut self,
        view: &SizeInfo,
        display_offset: usize,
        total_lines: usize,
    ) {
        let Some(geo) = self.scrollbar_geometry(view, display_offset, total_lines) else { return };
        let (thumb_x, thumb_y, thumb_w, thumb_h) = geo;

        // Skinned so it reads as chrome on both light and dark themes; a bit
        // more opaque while grabbed so the drag has visible feedback.
        let alpha = if self.nebula_scrollbar_drag.is_some() { 0.62 } else { 0.40 };
        let thumb_color = self.nebula_theme.skin().scrollbar_thumb.with_alpha(alpha);
        let quad = UiQuad::solid(thumb_x, thumb_y, thumb_w, thumb_h, thumb_w * 0.5, thumb_color);
        self.renderer.draw_ui(&self.size_info, &[quad]);
    }

    /// Scrollbar thumb rect `(x, y, w, h)` for a pane `view` — the single
    /// source of truth shared by rendering and input hit-testing. `None` while
    /// the bar is hidden (at the bottom, or no history).
    fn scrollbar_geometry(
        &self,
        view: &SizeInfo,
        display_offset: usize,
        total_lines: usize,
    ) -> Option<(f32, f32, f32, f32)> {
        let screen_lines = view.screen_lines();
        // Nothing to show when sitting at the bottom or when there's no history.
        if display_offset == 0 || total_lines <= screen_lines {
            return None;
        }

        let scale = self.window.scale_factor as f32;
        let total = total_lines as f32;
        let track_top = view.padding_y();
        let track_h = screen_lines as f32 * view.cell_height();
        if track_h <= 1.0 {
            return None;
        }

        // Thumb height = visible fraction of total content, with a sane minimum.
        let min_thumb = (24.0 * scale).min(track_h);
        let thumb_h = (track_h * (screen_lines as f32 / total)).clamp(min_thumb, track_h);

        // Lines of history above the current viewport top (0 = top, history = bottom).
        let history = total_lines - screen_lines;
        let above = (history - display_offset) as f32;
        let max_y = (track_h - thumb_h).max(0.0);
        let thumb_y = track_top + (track_h * (above / total)).clamp(0.0, max_y);

        // Float over the grid's right edge (overlay style, like macOS scrollbars).
        let thumb_w = (4.0 * scale).max(2.0);
        let grid_right = view.padding_x() + view.columns() as f32 * view.cell_width();
        let thumb_x = grid_right - thumb_w;

        Some((thumb_x, thumb_y, thumb_w, thumb_h))
    }

    /// Hit-test a press against the scrollbar. The 4px thumb gets a widened
    /// grab zone; a hit returns the pointer's y-offset inside the thumb so the
    /// drag doesn't jump. A press on the track (above/below the thumb) recenters
    /// the thumb there (`grab = thumb_h / 2`).
    pub fn scrollbar_grab(
        &self,
        view: &SizeInfo,
        display_offset: usize,
        total_lines: usize,
        x: f32,
        y: f32,
    ) -> Option<f32> {
        let (thumb_x, thumb_y, thumb_w, thumb_h) =
            self.scrollbar_geometry(view, display_offset, total_lines)?;
        let scale = self.window.scale_factor as f32;
        let slop = 8.0 * scale;
        // Horizontal band around the thumb column.
        if x < thumb_x - slop || x > thumb_x + thumb_w + slop {
            return None;
        }
        // Vertical: inside the track at all?
        let track_top = view.padding_y();
        let track_h = view.screen_lines() as f32 * view.cell_height();
        if y < track_top || y > track_top + track_h {
            return None;
        }
        if y >= thumb_y && y <= thumb_y + thumb_h {
            Some(y - thumb_y) // grab inside the thumb
        } else {
            Some(thumb_h / 2.0) // track press: jump so the thumb centers on it
        }
    }

    /// Map a dragged pointer `y` back to a scrollback `display_offset`,
    /// inverting the thumb-position math (`grab` = offset captured at press).
    pub fn scrollbar_target_offset(
        &self,
        view: &SizeInfo,
        total_lines: usize,
        y: f32,
        grab: f32,
    ) -> usize {
        let screen_lines = view.screen_lines();
        let history = total_lines.saturating_sub(screen_lines);
        if history == 0 {
            return 0;
        }
        let track_top = view.padding_y();
        let track_h = (screen_lines as f32 * view.cell_height()).max(1.0);
        let above = ((y - grab - track_top) / track_h * total_lines as f32).round();
        let above = above.clamp(0.0, history as f32) as usize;
        history - above
    }
}
