//! Window surface and frame presentation: the background image and window
//! backdrop painting, the GL context make-current/swap-buffers plumbing, the
//! transparency and font-size/glyph-cache refresh, frame presentation, the
//! focused-pane geometry accessor, and the `FrameTimer` vsync-paced timeout
//! computation.

use std::mem::ManuallyDrop;
use std::ops::Deref;
use std::path::Path;
use std::time::{Duration, Instant};

use glutin::config::GetGlConfig;
use glutin::context::PossiblyCurrentContext;
use glutin::display::GetGlDisplay;
use glutin::error::ErrorKind;
use glutin::prelude::*;
use glutin::surface::{Surface, SwapInterval, WindowSurface};

use log::{debug, info};
use winit::raw_window_handle::RawWindowHandle;

use super::panel_layout::apply_min_window_size;
use super::settings;
use super::surface_opacity;
use super::{Display, FrameTimer, SizeInfo, UI_SHELL_RADIUS_LOGICAL, compute_cell_size};
use super::{chrome, context_menu, nebula_debug_log, toast};

use crate::config::UiConfig;
use crate::config::font::Font;
use crate::display::color::Rgb;
use crate::renderer::ui::{Rgba, UiQuad};
use crate::renderer::{GlyphCache, Renderer, platform};
use crate::scheduler::Scheduler;

impl Display {
    fn draw_background_image(&mut self) {
        let Some(path) = self.nebula_background_image.as_deref() else {
            return;
        };
        let path = path.trim().trim_matches('"');
        if path.is_empty() {
            return;
        }

        // Keep PNG wallpaper loading in the renderer cache. The setting stores a
        // user path verbatim (usually `D:\...` on Windows); `cover` scaling and
        // alpha are handled by the image renderer.
        let target = if self.nebula_background_image_cover_chrome {
            (0.0, 0.0, self.size_info.width(), self.size_info.height())
        } else {
            self.terminal_card_rect()
        };
        // 卡片模式必须携带卡片圆角：矩形壁纸盖上去会吃掉终端卡的圆角。
        let clip_radius = if self.nebula_background_image_cover_chrome {
            0.0
        } else {
            (UI_SHELL_RADIUS_LOGICAL * self.window.scale_factor as f32).round()
        };
        self.renderer.draw_background_image(
            &self.size_info,
            Path::new(path),
            self.nebula_background_image_opacity,
            self.nebula_background_image_fit,
            self.nebula_background_image_alignment,
            target,
            target,
            clip_radius,
        );
    }

    /// Compose the stable frame backdrop once per frame.
    ///
    /// 层模型（2026-07-24 修订）：清屏完全透明，终端卡底永远先铺主题底
    /// 色（用户透明度），壁纸再以自身不透明度叠在其上——降低壁纸不透
    /// 明度时图片淡向主题底色（浅色主题→白、深色主题→黑），而不是透出
    /// 窗口后面的桌面（旧模型有壁纸时不画卡底，深色主题下低不透明度会
    /// 透出刺眼的白）。卡以外的壳由 chrome pass 的一体化壳层负责（同一
    /// 用户透明度）。
    /// 壳层合成色：panel 预合成在 shell_bg 上（保住面板 token 的调子），
    /// alpha 直接取用户不透明度。chrome 的条带与 backdrop 的凹角/清屏兜底
    /// **必须同源**取这一个值——各算各的迟早漂出色差接缝。
    pub(crate) fn shell_frame_color(&self) -> Rgba {
        let palette = self.nebula_theme.palette();
        let shell_alpha =
            surface_opacity::SurfaceOpacityPolicy::new(self.nebula_window_opacity).chrome;
        let pa = palette.panel.a as f32 / 255.0;
        let comp = |p: u8, b: u8| (p as f32 * pa + b as f32 * (1.0 - pa)).round() as u8;
        Rgba::new(
            comp(palette.panel.r, palette.shell_bg.r),
            comp(palette.panel.g, palette.shell_bg.g),
            comp(palette.panel.b, palette.shell_bg.b),
            (shell_alpha * 255.0).round().clamp(0.0, 255.0) as u8,
        )
    }

    pub(super) fn draw_window_backdrop(&mut self, terminal_background: Rgb) {
        // rgb 兜底取壳合成色（panel-over-shell_bg）：不透明窗口下 DWM 忽略
        // alpha，尚未被壳/卡覆盖的像素本来就在壳区，取纯 shell_bg 会比
        // 条带暗一档，正是四角亮线里混进的那个杂色。
        let shell = self.shell_frame_color();
        self.renderer.clear(Rgb::new(shell.r, shell.g, shell.b), 0.0);
        {
            let (card_x, card_y, card_w, card_h) = self.terminal_card_rect();
            let scale = self.window.scale_factor as f32;
            let alpha = (self.nebula_window_opacity * 255.0).round().clamp(0.0, 255.0) as u8;
            // 与 chrome 壳层同径同 round：半径差出小数像素就是一圈错位细缝。
            let radius =
                (UI_SHELL_RADIUS_LOGICAL * scale).round().min(card_w * 0.5).min(card_h * 0.5);
            // 2026-08-09 白角根因修复：凹角补片从 chrome 壳层挪到这里、画在
            // 卡片**之前**。原先卡与补片是两条独立 AA 弧按顺序 over，弧上
            // 必然残留 i(1-i)·清屏色 的交叉项——四角浮出一圈亮线，透明窗
            // 直接漏桌面。补片先把角块铺满壳色，卡的凸圆角向「已铺满的壳」
            // 过渡，成为唯一可见 AA 边，交叉项从结构上消失。
            let mut quads = Vec::with_capacity(5);
            if radius > 0.0 && card_w > 0.0 && card_h > 0.0 {
                quads.push(UiQuad::concave_corner(card_x, card_y, radius, 0, shell));
                quads.push(UiQuad::concave_corner(
                    card_x + card_w - radius,
                    card_y,
                    radius,
                    1,
                    shell,
                ));
                quads.push(UiQuad::concave_corner(
                    card_x + card_w - radius,
                    card_y + card_h - radius,
                    radius,
                    2,
                    shell,
                ));
                quads.push(UiQuad::concave_corner(
                    card_x,
                    card_y + card_h - radius,
                    radius,
                    3,
                    shell,
                ));
            }
            quads.push(UiQuad::solid(
                card_x,
                card_y,
                card_w,
                card_h,
                radius,
                Rgba::new(
                    terminal_background.r,
                    terminal_background.g,
                    terminal_background.b,
                    alpha,
                ),
            ));
            self.renderer.draw_ui(&self.size_info, &quads);
        }

        // The image is intentionally independent of the terminal tint: its own
        // opacity means image strength, not a value that disappears at 100%
        // terminal opacity.
        self.draw_background_image();
    }

    /// Whether a wallpaper path is configured for the terminal card.
    fn has_background_image(&self) -> bool {
        self.nebula_background_image
            .as_deref()
            .map(|p| !p.trim().trim_matches('"').is_empty())
            .unwrap_or(false)
    }

    /// Sync the OS transparency flag with the user opacity slider. 壁纸不
    /// 透明度不再参与：卡底永远先铺主题底色，壁纸变淡是淡向主题色而非
    /// 透出窗口后面的桌面。
    pub(super) fn update_window_transparency(&mut self) {
        let transparent = self.nebula_window_opacity < 1.0;
        self.window.set_transparent(transparent);
        #[cfg(target_os = "macos")]
        self.window.set_has_shadow(!transparent);
    }

    #[inline]
    pub fn gl_context(&self) -> &PossiblyCurrentContext {
        &self.context
    }

    pub fn make_not_current(&mut self) {
        if self.context.is_current() {
            self.context.make_not_current_in_place().expect("failed to disable context");
        }
    }

    pub fn make_current(&mut self) {
        let is_current = self.context.is_current();

        // Attempt to make the context current if it's not.
        let context_loss = if is_current {
            self.renderer.was_context_reset()
        } else {
            match self.context.make_current(&self.surface) {
                Err(err) if err.error_kind() == ErrorKind::ContextLost => {
                    info!("Context lost for window {:?}", self.window.id());
                    true
                },
                _ => false,
            }
        };

        if !context_loss {
            return;
        }

        let gl_display = self.context.display();
        let gl_config = self.context.config();
        let raw_window_handle = Some(self.window.raw_window_handle());
        let context = platform::create_gl_context(&gl_display, &gl_config, raw_window_handle)
            .expect("failed to recreate context.");

        // Drop the old context and renderer.
        unsafe {
            ManuallyDrop::drop(&mut self.renderer);
            ManuallyDrop::drop(&mut self.context);
        }

        // Activate new context.
        let context = context.treat_as_possibly_current();
        self.context = ManuallyDrop::new(context);
        self.context.make_current(&self.surface).expect("failed to reativate context after reset.");

        // Recreate renderer.
        let renderer = Renderer::new(&self.context, self.renderer_preference)
            .expect("failed to recreate renderer after reset");
        self.renderer = ManuallyDrop::new(renderer);

        // Resize the renderer.
        self.renderer.resize(&self.size_info);

        self.reset_glyph_cache();
        self.damage_tracker.frame().mark_fully_damaged();

        debug!("Recovered window {:?} from gpu reset", self.window.id());
    }

    fn swap_buffers(&self) {
        #[allow(clippy::single_match)]
        let res = match (self.surface.deref(), &self.context.deref()) {
            #[cfg(not(any(target_os = "macos", windows)))]
            (Surface::Egl(surface), PossiblyCurrentContext::Egl(context))
                if matches!(self.raw_window_handle, RawWindowHandle::Wayland(_))
                    && !self.damage_tracker.debug =>
            {
                let damage = self.damage_tracker.shape_frame_damage(self.size_info.into());
                surface.swap_buffers_with_damage(context, &damage)
            },
            (surface, context) => surface.swap_buffers(context),
        };
        if let Err(err) = res {
            debug!("error calling swap_buffers: {err}");
        }
    }

    /// Update font size and cell dimensions.
    ///
    /// This will return a tuple of the cell width and height.
    pub(super) fn update_font_size(
        glyph_cache: &mut GlyphCache,
        config: &UiConfig,
        font: &Font,
        cell_width_mode: settings::CellWidthMode,
    ) -> (f32, f32) {
        let _ = glyph_cache.update_font_size(font);

        // Compute new cell sizes.
        let cell_dimensions =
            compute_cell_size(config, &glyph_cache.font_metrics(), cell_width_mode);

        // The built-in box-drawing / Powerline glyphs fill exactly the
        // effective cell width; pin it so they stop re-flooring the advance
        // (a 1px drift under the relaxed mode that splits lines into dashes).
        glyph_cache.set_cell_width(cell_dimensions.0 as usize);

        cell_dimensions
    }

    /// Re-derive the OS-enforced window floor from the current cell size and
    /// chrome, so the grid can never be dragged below
    /// [`SizeInfo::MIN_USABLE_COLUMNS`].
    ///
    /// Must be re-applied whenever the cell size or sidebar width changes: a
    /// floor computed for a 7px cell stops protecting anything once the user
    /// zooms to a 21px one.
    #[cfg(windows)]
    pub(super) fn apply_min_window_size(
        &self,
        config: &UiConfig,
        cell_width: f32,
        cell_height: f32,
    ) {
        apply_min_window_size(&self.window, config, cell_width, cell_height, self.nebula_sidebar_w);
    }

    /// Reset glyph cache.
    pub(super) fn reset_glyph_cache(&mut self) {
        let cache = &mut self.glyph_cache;
        self.renderer.with_loader(|mut api| {
            cache.reset_glyph_cache(&mut api);
        });
    }

    pub(super) fn present_frame(&mut self, scheduler: &mut Scheduler) {
        // 本帧的 UI 锚定比率：chrome/设置/浮层文本按它反向补偿终端缩放。
        // 终端网格与文档正文不经过 chrome-text 路径，不受影响。
        let ui_scale = self.ui_text_scale();
        self.renderer.set_ui_text_scale(ui_scale);
        nebula_debug_log(format!(
            "render_present window={}x{} pane_view={} frame_images={} chrome_logos={}",
            self.size_info.width(),
            self.size_info.height(),
            self.nebula_pane_view.is_some(),
            self.nebula_frame_images.len(),
            self.nebula_chrome_logo_draws.len(),
        ));
        // OSC 1337 inline images collected by the pane passes: draw above the
        // cells, below the chrome/modals.
        if !self.nebula_frame_images.is_empty() {
            let size = self.size_info;
            let images = std::mem::take(&mut self.nebula_frame_images);
            for (id, rgba, px, rect) in &images {
                self.renderer.draw_inline_image(&size, *id, rgba, *px, *rect);
            }
        }

        // Draw Nebula window chrome (title bar and tab sidebar).
        chrome::draw_chrome(self);

        // AI brand logos staged by the chrome pass: drawn only now, after the
        // last chrome text flush, because draw_inline_image's viewport/blend
        // round-trip poisons any glyph batch that follows it.
        if !self.nebula_chrome_logo_draws.is_empty() {
            let size = self.size_info;
            let logos = std::mem::take(&mut self.nebula_chrome_logo_draws);
            for (id, rgba, px, rect) in &logos {
                self.renderer.draw_inline_image(&size, *id, rgba, *px, *rect);
            }
        }

        // SSH 连接卡片：在 chrome 之上，resize HUD 之下。
        self.draw_ssh_connect();

        // Transient resize HUD painted on top of the chrome.
        self.draw_resize_hud();
        context_menu::draw(self);
        self.draw_ssh_delete_undo();
        // 消息栏的关闭按钮：横幅由终端 pass 画，按钮必须在它之上。
        self.draw_message_close();
        // 轻提示：右下角，在浮条之上、模态之下（模态要求决策，不该被提示压住）。
        toast::draw(self);
        self.draw_ai_fix_bar();
        self.draw_ssh_editor_modal();
        self.draw_confirm_modal();

        // Notify winit that we're about to present.
        self.window.pre_present_notify();

        // Highlight damage for debugging.
        if self.damage_tracker.debug {
            let metrics = self.glyph_cache.font_metrics();
            let damage = self.damage_tracker.shape_frame_damage(self.size_info.into());
            let mut rects = Vec::with_capacity(damage.len());
            self.highlight_damage(&mut rects);
            self.renderer.draw_rects(&self.size_info, &metrics, rects);
        }

        // Clearing debug highlights from the previous frame requires full redraw.
        self.swap_buffers();

        if matches!(self.raw_window_handle, RawWindowHandle::Xcb(_) | RawWindowHandle::Xlib(_)) {
            // On X11 `swap_buffers` does not block for vsync. However the next OpenGl command
            // will block to synchronize (this is `glClear` in Nebula), which causes a
            // permanent one frame delay.
            self.renderer.finish();
        }

        // XXX: Request the new frame after swapping buffers, so the
        // time to finish OpenGL operations is accounted for in the timeout.
        if !matches!(self.raw_window_handle, RawWindowHandle::Wayland(_)) {
            self.request_frame(scheduler);
        }

        self.damage_tracker.swap_damage();
    }

    /// Geometry that input and hint hit-testing should use: the focused pane's
    /// half-width view when a split is active, otherwise the full window.
    #[inline]
    pub fn pane_view(&self) -> SizeInfo {
        self.nebula_pane_view.unwrap_or(self.size_info)
    }
}

impl FrameTimer {
    pub fn new() -> Self {
        let now = Instant::now();
        Self { base: now, last_synced_timestamp: now, refresh_interval: Duration::ZERO }
    }

    /// Compute the delay that we should use to achieve the target frame
    /// rate.
    pub fn compute_timeout(&mut self, refresh_interval: Duration) -> Duration {
        let now = Instant::now();

        // Handle refresh rate change.
        if self.refresh_interval != refresh_interval {
            self.base = now;
            self.last_synced_timestamp = now;
            self.refresh_interval = refresh_interval;
            return refresh_interval;
        }

        let next_frame = self.last_synced_timestamp + self.refresh_interval;

        if next_frame < now {
            // Redraw immediately if we haven't drawn in over `refresh_interval` microseconds.
            let elapsed_micros = (now - self.base).as_micros() as u64;
            let refresh_micros = self.refresh_interval.as_micros() as u64;
            self.last_synced_timestamp =
                now - Duration::from_micros(elapsed_micros % refresh_micros);
            Duration::ZERO
        } else {
            // Redraw on the next `refresh_interval` clock tick.
            self.last_synced_timestamp = next_frame;
            next_frame - now
        }
    }
}
