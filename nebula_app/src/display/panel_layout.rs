//! Layout constants, panel drag, zoom/font refresh and sidebar interaction.

use std::mem;
use std::time::Duration;

use crossfont::Size as FontSize;
use winit::dpi::{LogicalSize, PhysicalSize};

use nebula_terminal::term::MIN_SCREEN_LINES;

use super::chrome::{self, chrome_tab_layout, contains_rect};
use super::settings;
use super::ui;
use super::{NebulaTheme, SizeInfo, compute_cell_size};
use super::{nebula_data_dir};

use crate::config::UiConfig;

use super::Display;

/// Top chrome reserve, in logical pixels at scale factor 1.0. Sized as: top
/// bar (8 margin + 40 bar) + card seam (8) + 8px of breathing room inside the
/// terminal card, so the first grid row doesn't touch the card's top edge.
pub const CHROME_BAR_LOGICAL: f32 = 64.0;

/// 「刚完成」对勾在徽章位上停留多久，随后落回未读圆点。
///
/// 短到不像一个需要处理的状态、长到能被余光捕捉：低于 ~0.6s 在扫视中会被
/// 整个错过，高于 ~2s 就开始像"它卡在完成态上了"。
pub(crate) const BADGE_FLASH: std::time::Duration = std::time::Duration::from_millis(1100);

/// Shared chrome/control corner radius. Used for the small in-shell affordances
/// (window-control hover pills, tab pills, the "+" square) — kept modest so the
/// controls stay crisp.
///
/// `pub(crate)`: the GPUI shell maps this onto `gpui_component::Theme::radius`
/// so its controls share the legacy pill curve.
pub(crate) const UI_CORNER_RADIUS_LOGICAL: f32 = 8.0;

/// Outer radius of the connected chrome shell (the L-frame formed by the top
/// bar + left sidebar). Larger than the control radius so the whole window
/// chrome reads as one soft-cornered card while the affordances inside keep
/// their tighter [`UI_CORNER_RADIUS_LOGICAL`] curve.
///
/// `pub(crate)`: the GPUI shell's terminal card borrows this exact value
/// (see `gpui_shell::theme::card_radius`) so both shells round the card
/// identically.
///
/// The number itself lives in `nebula_settings` — it is the default for the
/// per-theme card geometry, and a second literal here would be exactly the
/// "two copies of one number" that produced the white seam around the card.
pub(crate) const UI_SHELL_RADIUS_LOGICAL: f32 = nebula_settings::DEFAULT_PANE_CARD_RADIUS;

/// Gap between the terminal card and the window's right/bottom edges, in
/// logical pixels — the visible "seam" of shell color that makes the terminal
/// read as a rounded card floating on the shell backdrop. Top and left carry
/// no seam of their own: the card tucks up under the top bar and sidebar.
pub(crate) const UI_CARD_SEAM_LOGICAL: f32 = 8.0;

/// Shared quiet outline thickness.
pub(crate) const UI_HAIRLINE_LOGICAL: f32 = 1.0;

/// Horizontal breathing space for terminal content, in logical pixels.
/// Kept modest so the grid stays wide — this is *added on top of* the user's
/// configured `window.padding`, on both sides, so large values noticeably
/// narrow the usable area.
pub const CONTENT_PAD_X_LOGICAL: f32 = 20.0;

/// Reserved chrome height per side, in physical pixels for `scale_factor`.
#[inline]
pub fn chrome_reserve(scale_factor: f32) -> f32 {
    (CHROME_BAR_LOGICAL * scale_factor).round()
}

/// Bottom grid reserve: card seam plus the same 8px inner breathing room used
/// above the first row. Unlike [`chrome_reserve`], there is no title bar below
/// the terminal, so mirroring the 64px top reserve creates a large dead band.
#[inline]
pub fn bottom_content_reserve(scale_factor: f32) -> f32 {
    ((UI_CARD_SEAM_LOGICAL + 8.0) * scale_factor).round()
}

/// Horizontal content padding, in physical pixels for `scale_factor`.
#[inline]
pub fn content_pad_x(scale_factor: f32) -> f32 {
    (CONTENT_PAD_X_LOGICAL * scale_factor).round()
}

/// Width of the left tab sidebar when expanded, in logical pixels. Chosen to
/// match the reference design — wide enough for a directory-ish label plus a
/// close affordance, narrow enough to leave the grid roomy.
pub const SIDEBAR_W_LOGICAL: f32 = 230.0;

/// 拖拽调节（设置·交互开关）允许的范围，逻辑 px。下限保行内容可读，
/// 上限防把终端挤成一条缝；settings 解析与拖拽 update 用同一组钳制，
/// 手拖出来的值和手改文件写出来的值才不会各有一套边界。
pub const SIDEBAR_W_MIN: f32 = 170.0;
pub const SIDEBAR_W_MAX: f32 = 420.0;
pub const DRAWER_W_MIN: f32 = 220.0;
pub const DRAWER_W_MAX: f32 = 560.0;
/// SSH HOSTS 停靠区高度覆盖的下限 = 只剩标题条（`hosts_header_h` 的逻辑值）。
pub const HOSTS_BAND_MIN: f32 = 38.0;

/// Sidebar width in physical pixels for `scale_factor`, honouring the collapsed
/// state. `logical_w` 是当前（可能被拖拽调过的）逻辑宽，[`SIDEBAR_W_LOGICAL`]
/// 只是它的默认值。
#[inline]
pub fn sidebar_width(scale_factor: f32, collapsed: bool, logical_w: f32) -> f32 {
    if collapsed { 0.0 } else { (logical_w * scale_factor).round() }
}

/// Re-derive the OS-enforced window floor from the current cell size and
/// chrome, so the grid can never be dragged below
/// [`SizeInfo::MIN_USABLE_COLUMNS`].
///
/// Must be re-applied whenever the cell size or sidebar width changes: a floor
/// computed for a 7px cell stops protecting anything once the user zooms to a
/// 21px one. `set_min_inner_size` is logical DIPs, so the physical paddings are
/// divided back out by the scale factor.
#[cfg(windows)]
pub(crate) fn apply_min_window_size(
    window: &crate::display::window::Window,
    config: &UiConfig,
    cell_width: f32,
    cell_height: f32,
    sidebar_logical_w: f32,
) {
    let scale = window.scale_factor as f32;
    let pad = config.window.padding(scale);
    let content_pad = content_pad_x(scale);
    let min_w = SizeInfo::min_usable_width(
        cell_width,
        pad.0 + content_pad + sidebar_width(scale, false, sidebar_logical_w),
        pad.0 + content_pad,
    );
    let min_h = SizeInfo::min_usable_height(
        cell_height,
        pad.1 + chrome_reserve(scale),
        pad.1,
        nebula_terminal::term::MIN_SCREEN_LINES,
    );
    window
        .set_min_inner_size(Some(LogicalSize::new((min_w / scale) as f64, (min_h / scale) as f64)));
}

/// 三条可拖拽的面板分界线（设置·交互的「拖拽调节」开关管辖）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelDragKind {
    /// 左侧栏右缘：拖宽度。
    SidebarWidth,
    /// SSH HOSTS 停靠区顶缘：拖高度。
    HostsBand,
    /// 右抽屉左缘：拖宽度。
    DrawerWidth,
}

/// 进行中的面板拖拽。侧栏/抽屉的宽度变化会重排终端（PTY 端另有 settle
/// 延迟），所以应用端按 [`PANEL_DRAG_REFLOW_MS`] 节流：**视觉**几何每帧
/// 跟手（chrome/抽屉布局读 `target`），**reflow** 用的已应用字段到点才
/// 同步，松手必同步——拖动过程平滑，网格重排最多 12 次/秒。
#[derive(Debug, Clone, Copy)]
pub struct PanelDrag {
    pub kind: PanelDragKind,
    /// 拖动中的目标值（逻辑 px）：宽度或停靠区高度。
    pub target: f32,
    /// 上次把 `target` 同步进已应用字段的时刻。
    pub last_apply: std::time::Instant,
    /// HOSTS 分界专用：按下时缓存的停靠区内容底缘（物理 px）。它只取决于
    /// 面板几何、与停靠区自身高度无关，所以整场拖拽都不会变——缓存下来，
    /// 每次指针移动就不必重跑一遍 `chrome_tab_layout`。
    pub anchor: f32,
}

/// 拖动期间两次终端 reflow 的最小间隔（毫秒）。
pub const PANEL_DRAG_REFLOW_MS: u64 = 80;

/// 侧栏拖到比这更窄（逻辑 px）就直接收起，而不是卡在 [`SIDEBAR_W_MIN`]。
/// 用户裁定：下限的语义是「关掉」不是「最窄」——把边界一路推到左边缘是
/// 最自然的收起手势。宽度字段保持折叠前的值，重新展开还是原来那么宽。
pub const SIDEBAR_COLLAPSE_AT: f32 = 120.0;

/// 右抽屉的同款阈值：拖到比这更窄就关掉抽屉。两侧手势必须对称，否则
/// 「左边拖到头会关、右边拖到头只是卡住」本身就是个 bug（用户 08-02 报）。
pub const DRAWER_COLLAPSE_AT: f32 = 150.0;

impl Display {
    /// Fold the tab sidebar in or out. Toggling changes the grid's usable width,
    /// so it re-runs the resize/reflow path by re-feeding the current window
    /// size — `handle_update` then recomputes the asymmetric padding split.
    pub fn toggle_sidebar(&mut self) {
        self.nebula_sidebar_collapsed = !self.nebula_sidebar_collapsed;
        let size = PhysicalSize::new(self.size_info.width() as u32, self.size_info.height() as u32);
        self.pending_update.set_dimensions(size);
        self.window.request_redraw();
        self.pending_update.dirty = true;
    }

    /// 侧栏的**视觉**逻辑宽：拖动中读 target（每帧跟手），否则读已应用值。
    /// chrome 布局与卡片几何用它；reflow（`handle_update` 的 padding）只认
    /// `nebula_sidebar_w`——两者的差就是节流窗口内允许的短暂错位。
    pub(super) fn sidebar_w_visual(&self) -> f32 {
        match self.nebula_panel_drag {
            Some(d) if d.kind == PanelDragKind::SidebarWidth => d.target,
            _ => self.nebula_sidebar_w,
        }
    }

    /// 右抽屉的视觉逻辑宽（同 [`Self::sidebar_w_visual`] 的拖动语义）。
    pub(super) fn drawer_w_visual(&self) -> f32 {
        match self.nebula_panel_drag {
            Some(d) if d.kind == PanelDragKind::DrawerWidth => d.target,
            _ => self.nebula_drawer_w,
        }
    }

    /// 指针是否落在三条可拖分界线之一。热区 ±4 逻辑 px；动画进行中不给热区
    /// ——滑动中的边缘抓不准。
    ///
    /// 两条**宽度**分界（侧栏右缘、抽屉左缘）要拖动会重排终端，归「拖拽调节」
    /// 开关管；SSH HOSTS 分界只在侧栏内部分配高度，不碰网格，所以默认就能拖，
    /// 不受开关约束（用户 08-02 裁定）。
    pub fn panel_resize_hit(&self, x: f32, y: f32) -> Option<PanelDragKind> {
        let scale = self.window.scale_factor as f32;
        let grip = 4.0 * scale;
        if self.nebula_panel_resize
            && self.side_panel_visible()
            && self.nebula_ui_anims.right_drawer.value() > 0.996
        {
            let (px, py, _, ph) = self.side_panel_layout().panel;
            if y >= py && y <= py + ph && (x - px).abs() <= grip {
                return Some(PanelDragKind::DrawerWidth);
            }
        }
        if self.left_sidebar_visible() && self.left_sidebar_progress() > 0.996 {
            let layout =
                chrome::chrome_tab_layout(&self.ui_size_info(), scale, self.sidebar_model(), 1.0);
            let (px, py, pw, ph) = layout.panel;
            if pw > 0.0 {
                if self.nebula_panel_resize
                    && y >= py
                    && y <= py + ph
                    && (x - (px + pw)).abs() <= grip
                {
                    return Some(PanelDragKind::SidebarWidth);
                }
                // HOSTS 分界 = 停靠区标题条的顶缘。
                let (hx, hy, hw, _) = layout.hosts_header;
                if self.nebula_hosts_section_open
                    && hw > 0.0
                    && x >= hx
                    && x <= hx + hw
                    && (y - hy).abs() <= grip
                {
                    return Some(PanelDragKind::HostsBand);
                }
            }
        }
        None
    }

    /// input 层在分界线上按下时开启一场拖拽。
    pub fn begin_panel_drag(&mut self, kind: PanelDragKind) {
        let target = match kind {
            PanelDragKind::SidebarWidth => self.nebula_sidebar_w,
            PanelDragKind::DrawerWidth => self.nebula_drawer_w,
            PanelDragKind::HostsBand => self.nebula_hosts_band.max(HOSTS_BAND_MIN),
        };
        let anchor = if kind == PanelDragKind::HostsBand {
            let scale = self.window.scale_factor as f32;
            chrome::chrome_tab_layout(&self.ui_size_info(), scale, self.sidebar_model(), 1.0)
                .dock_content_bottom
        } else {
            0.0
        };
        self.nebula_panel_drag =
            Some(PanelDrag { kind, target, last_apply: std::time::Instant::now(), anchor });
    }

    /// 拖动中的指针移动：换算目标值。HOSTS 分界纯 chrome 内部、即时生效；
    /// 两个宽度分界的**视觉**几何每帧跟手，真正的 reflow 要同时满足两道闸
    /// ——[`PANEL_DRAG_REFLOW_MS`] 的时间节流，以及位移至少跨过一个单元格
    /// 宽度。后者才是重点：网格列数只在跨过整数列时才会变，同一列内反复
    /// reflow 是纯浪费。返回 true = 需要重绘。
    pub fn update_panel_drag(&mut self, x: f32, y: f32) -> bool {
        let scale = self.window.scale_factor as f32;
        let Some(drag) = self.nebula_panel_drag else { return false };
        let target = match drag.kind {
            // chrome_tab_layout：panel_x = margin(8)、panel_w = sw - 8 - 12，
            // 右缘 = sw - 12 ⇒ sw = x + 12（都在逻辑座标系里算）。
            PanelDragKind::SidebarWidth => {
                let raw = x / scale + 12.0;
                if raw < SIDEBAR_COLLAPSE_AT {
                    // 推到左边缘 = 收起。这场拖拽就此结束（侧栏没了，分界线
                    // 也就没了），宽度字段保持不动。
                    self.nebula_panel_drag = None;
                    if !self.nebula_sidebar_collapsed {
                        self.toggle_sidebar();
                    }
                    self.persist_nebula_settings();
                    return true;
                }
                raw.clamp(SIDEBAR_W_MIN, SIDEBAR_W_MAX)
            },
            PanelDragKind::DrawerWidth => {
                let w = (self.size_info.width() - 8.0 * scale - x) / scale;
                if w < DRAWER_COLLAPSE_AT {
                    // 推到右边缘 = 关掉抽屉，与侧栏拖到最左同一手势。宽度
                    // 字段不动，下次打开还是原来那么宽。不走 close_sftp_panel：
                    // 那条路会取消正在进行的传输，而这里只是把面板收起来。
                    self.nebula_panel_drag = None;
                    if self.nebula_side_panel.open {
                        self.nebula_side_panel.open = false;
                        let size = PhysicalSize::new(
                            self.size_info.width() as u32,
                            self.size_info.height() as u32,
                        );
                        self.pending_update.set_dimensions(size);
                    }
                    self.persist_nebula_settings();
                    self.pending_update.dirty = true;
                    self.window.request_redraw();
                    return true;
                }
                let cap = DRAWER_W_MAX.min(self.size_info.width() * 0.42 / scale);
                w.clamp(DRAWER_W_MIN.min(cap), cap)
            },
            PanelDragKind::HostsBand => ((drag.anchor - y) / scale).max(HOSTS_BAND_MIN),
        };
        // 已应用值：宽度类要用它判断这次位移够不够跨一个单元格。
        let applied = match drag.kind {
            PanelDragKind::SidebarWidth => self.nebula_sidebar_w,
            PanelDragKind::DrawerWidth => self.nebula_drawer_w,
            PanelDragKind::HostsBand => 0.0,
        };
        let cell_w = self.size_info.cell_width().max(1.0);
        let Some(drag) = self.nebula_panel_drag.as_mut() else { return false };
        if (target - drag.target).abs() < 0.5 {
            return false;
        }
        drag.target = target;
        let due = drag.last_apply.elapsed()
            >= std::time::Duration::from_millis(PANEL_DRAG_REFLOW_MS)
            && (target - applied).abs() * scale >= cell_w;
        match drag.kind {
            PanelDragKind::HostsBand => self.nebula_hosts_band = target,
            PanelDragKind::SidebarWidth | PanelDragKind::DrawerWidth if due => {
                drag.last_apply = std::time::Instant::now();
                self.apply_panel_drag_target();
            },
            _ => {},
        }
        self.pending_update.dirty = true;
        self.window.request_redraw();
        true
    }

    /// 把 target 同步进已应用字段，宽度类走 toggle_sidebar 同款 reflow 触发。
    fn apply_panel_drag_target(&mut self) {
        let Some(drag) = self.nebula_panel_drag else { return };
        match drag.kind {
            PanelDragKind::SidebarWidth => self.nebula_sidebar_w = drag.target,
            PanelDragKind::DrawerWidth => self.nebula_drawer_w = drag.target,
            PanelDragKind::HostsBand => {
                self.nebula_hosts_band = drag.target;
                return;
            },
        }
        let size = PhysicalSize::new(self.size_info.width() as u32, self.size_info.height() as u32);
        self.pending_update.set_dimensions(size);
    }

    /// 松开：最终应用 + 持久化。返回 true = 确有一场拖拽在收尾。
    pub fn end_panel_drag(&mut self) -> bool {
        if self.nebula_panel_drag.is_none() {
            return false;
        }
        self.apply_panel_drag_target();
        self.nebula_panel_drag = None;
        self.persist_nebula_settings();
        self.pending_update.dirty = true;
        self.window.request_redraw();
        true
    }

    /// DPI 变化时按同一比例重标 UI 角色字号（等价于配置字号 × 新缩放）。
    /// Apply a monitor scale change after any native move transaction has
    /// settled. Keeping this in Display makes the immediate and deferred paths
    /// use exactly the same font/UI invalidation sequence.
    pub(crate) fn apply_scale_factor_change(&mut self, scale_factor: f64, config: &UiConfig) {
        let old_scale_factor = mem::replace(&mut self.window.scale_factor, scale_factor);
        if (old_scale_factor - scale_factor).abs() <= f64::EPSILON {
            return;
        }

        let font_scale = scale_factor as f32 / old_scale_factor as f32;
        self.font_size = self.font_size.scale(font_scale);
        self.rescale_ui_font(font_scale);

        let font = self.effective_font(&config.font);
        let font_size = self.font_size;
        self.pending_update.set_font(font.with_size(font_size));
    }

    pub(crate) fn rescale_ui_font(&mut self, factor: f32) {
        if factor.is_finite() && factor > 0.0 {
            self.nebula_ui_font.px *= factor;
        }
    }

    /// UI 角色字号相对当前终端字号的比率。仅供尚未角色化的历史缩放路径
    /// （`begin_chrome_text_scaled`、链接预览的手动锚定）使用；新代码一律
    /// 走 `draw_chrome_text*` / `draw_ui_text*`，它们直接从字体角色取真实
    /// 字号与 metrics。
    pub(crate) fn ui_text_scale(&self) -> f32 {
        let cur = self.font_size.as_px();
        if cur <= 0.0 || self.nebula_ui_font.px <= 0.0 {
            return 1.0;
        }
        self.nebula_ui_font.px / cur
    }

    /// [`Self::size_info`] 的 UI 版本：cell 尺寸来自 UI 字体角色的真实
    /// 栅格 metrics（[`Self::refresh_ui_font`] 量取）。chrome / 设置 /
    /// 浮层的布局与命中测试统一用它，与按同一角色栅格化的 UI 文本严格
    /// 同源；终端网格、damage、光标继续用原 `size_info`。
    pub(crate) fn ui_size_info(&self) -> SizeInfo {
        let mut ui = self.size_info;
        let (cell_w, cell_h) = self.nebula_ui_font.cell;
        ui.cell_width = cell_w;
        ui.cell_height = cell_h;
        ui
    }

    /// Pin the glyph cache's UI font role to the anchor size and refresh the
    /// cell the chrome layout steps by. Runs at construction and on every
    /// terminal font change — zoom, family, DPI all funnel through the font
    /// update, so this is the single place the role can go stale.
    pub(super) fn refresh_ui_font(&mut self, config: &UiConfig) {
        let ui_size = FontSize::from_px(self.nebula_ui_font.px);
        let metrics = self.glyph_cache.set_ui_font_size(ui_size);
        // 原生界面的字体单元格不受单元格宽度模式影响——该偏好只控制终端
        // 内容网格的列宽。这里固定用上游的向下取整。
        self.nebula_ui_font.cell =
            compute_cell_size(config, &metrics, settings::CellWidthMode::Compact);
        // 同步把 UI 域的生效列宽交给 glyph_cache，使 UI 文本里的内建
        // 字形（光标形状预览的 │ █ ▁）与 chrome 网格列宽对齐。
        self.glyph_cache.set_ui_cell_width(self.nebula_ui_font.cell.0 as usize);
        let ratio = self.ui_text_scale();
        // Unconditional breadcrumb (tiny, a handful of lines per session):
        // diagnosing "the sidebar zooms with the terminal" reports needs this
        // from USER instances, which never run with NEBULA_DEBUG_LOG set.
        let line = format!(
            "[{}] ui_anchor ratio={ratio:.3} ui_font_px={:.1} font_px={:.1} scale={:.2} term_cell={}x{} ui_cell={:?}\n",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            self.nebula_ui_font.px,
            self.font_size.as_px(),
            self.window.scale_factor,
            self.size_info.cell_width,
            self.size_info.cell_height,
            self.nebula_ui_font.cell
        );
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(nebula_data_dir().join("ui_anchor.log"))
        {
            use std::io::Write as _;
            let _ = file.write_all(line.as_bytes());
        }
    }

    /// Geometry of the rounded terminal card in physical pixels `(x, y, w, h)`.
    /// The card floats on the shell backdrop: flush-ish against the sidebar on
    /// the left and the top bar above (they share the shell color, so no seam
    /// is needed there), with a visible [`UI_CARD_SEAM_LOGICAL`] gap of shell
    /// color on the right and bottom edges. The grid's own padding
    /// (`content_pad_x` / `chrome_reserve`) is larger than the card inset, so
    /// all cell content lands inside the card.
    pub(crate) fn terminal_card_rect(&self) -> (f32, f32, f32, f32) {
        let scale = self.window.scale_factor as f32;
        let s = |v: f32| (v * scale).round();
        let seam = s(UI_CARD_SEAM_LOGICAL);
        // Left edge rides the sidebar's fold animation (same swift-out cubic
        // as the panel slide in `chrome_tab_layout`), so collapsing the
        // sidebar reads as the terminal card gliding left to claim the space
        // instead of snapping. Resting expanded: just past the sidebar
        // panel's right edge (`sw - 12` logical, see `chrome_tab_layout`);
        // resting collapsed: the chrome margin.
        let t = self.left_sidebar_progress().clamp(0.0, 1.0);
        let sw = (self.sidebar_w_visual() * scale).round();
        let x = s(8.0) + t * (sw - s(4.0) - s(8.0));
        // Top edge: the top bar's bottom (margin 8 + bar height 40, matching
        // `draw_chrome`), plus a seam so the card visibly floats below it.
        let y = s(8.0 + 40.0) + seam;
        // Right edge follows the file/git drawer the same way: as it slides
        // in, the card cedes its width (drawer width + margin) plus the seam.
        let dt = self.nebula_ui_anims.right_drawer.value().clamp(0.0, 1.0);
        let drawer =
            dt * ((self.drawer_w_visual() * scale).min(self.size_info.width() * 0.42) + s(8.0));
        let w = (self.size_info.width() - drawer - seam - x).max(0.0);
        let h = (self.size_info.height() - seam - y).max(0.0);
        (x, y, w, h)
    }

    pub fn side_panel_visible(&self) -> bool {
        self.nebula_ui_anims.right_drawer.visible(self.nebula_side_panel.open)
    }

    /// Sidebar content model for `chrome_tab_layout` — the single place the
    /// section states are read, so drawing / hit-testing / wheel agree.
    pub(super) fn sidebar_model(&self) -> chrome::SidebarModel {
        chrome::SidebarModel {
            tab_count: self.nebula_tab_labels.len().max(1),
            // Saved SSH destinations belong in the launcher/settings. The
            // home tab rail is reserved for actual sessions, so no second
            // SSH HOSTS section is laid out underneath TABS.
            host_count: 0,
            tabs_open: self.nebula_tabs_section_open,
            hosts_open: false,
            tabs_scroll: self.nebula_tabs_scroll,
            hosts_scroll: self.nebula_hosts_scroll,
            sidebar_w: self.sidebar_w_visual(),
            hosts_band: self.nebula_hosts_band,
        }
    }

    /// Toggle a sidebar section's accordion fold (click on its caption).
    pub fn toggle_sidebar_section(&mut self, hosts: bool) {
        if hosts {
            self.nebula_hosts_section_open = !self.nebula_hosts_section_open;
        } else {
            self.nebula_tabs_section_open = !self.nebula_tabs_section_open;
        }
        self.pending_update.dirty = true;
    }

    /// Toggle the queue entry now; the expanded panel will consume the same
    /// state in the next integration stage, so the entry's hit contract does
    /// not need to change when real queue content lands.
    pub fn toggle_message_queue_entry(&mut self) {
        self.nebula_message_queue_entry.toggle();
        self.pending_update.dirty = true;
        self.window.request_redraw();
    }

    /// Route a mouse-wheel tick over the sidebar into the section under the
    /// pointer. Returns true when consumed (pointer was over a section band).
    pub fn sidebar_wheel(&mut self, x: f32, y: f32, rows: i32) -> bool {
        if !self.left_sidebar_visible() {
            return false;
        }
        let layout = chrome_tab_layout(
            &self.ui_size_info(),
            self.window.scale_factor as f32,
            self.sidebar_model(),
            self.left_sidebar_progress(),
        );
        let (px, _, pw, _) = layout.panel;
        if pw <= 0.0 || x < px || x > px + pw {
            return false;
        }
        let scroll =
            |cur: usize, max: usize| -> usize { (cur as i32 + rows).clamp(0, max as i32) as usize };
        // Band membership includes each section's header so the wheel works
        // right up against the caption.
        if y >= layout.tabs_header.1 && y <= layout.tabs_band.1 {
            self.nebula_tabs_scroll = scroll(self.nebula_tabs_scroll, layout.tabs_max_scroll);
        } else if y >= layout.hosts_header.1
            && y <= layout.hosts_band.1.max(layout.hosts_header.1 + layout.hosts_header.3)
        {
            self.nebula_hosts_scroll = scroll(self.nebula_hosts_scroll, layout.hosts_max_scroll);
        } else {
            return false;
        }
        self.pending_update.dirty = true;
        true
    }

    pub fn sidebar_scrollbar_press(&mut self, x: f32, y: f32) -> bool {
        if !self.left_sidebar_visible() {
            return false;
        }
        let layout = chrome_tab_layout(
            &self.ui_size_info(),
            self.window.scale_factor as f32,
            self.sidebar_model(),
            self.left_sidebar_progress(),
        );
        let (kind, bar, max) =
            if let Some(bar) = layout.tabs_scrollbar.filter(|bar| bar.hit_test(x, y)) {
                (chrome::SidebarScrollKind::Tabs, bar, layout.tabs_max_scroll)
            } else if let Some(bar) = layout.hosts_scrollbar.filter(|bar| bar.hit_test(x, y)) {
                (chrome::SidebarScrollKind::Hosts, bar, layout.hosts_max_scroll)
            } else {
                return false;
            };
        let grab = if contains_rect(bar.thumb, x, y) { y - bar.thumb.1 } else { bar.thumb.3 * 0.5 };
        self.nebula_sidebar_scroll_drag = Some(chrome::SidebarScrollDrag { kind, grab });
        let target = bar.target_offset(y, grab, max);
        match kind {
            chrome::SidebarScrollKind::Tabs => self.nebula_tabs_scroll = target,
            chrome::SidebarScrollKind::Hosts => self.nebula_hosts_scroll = target,
        }
        self.pending_update.dirty = true;
        true
    }

    pub fn sidebar_scrollbar_drag_to(&mut self, y: f32) -> bool {
        let Some(drag) = self.nebula_sidebar_scroll_drag else { return false };
        let layout = chrome_tab_layout(
            &self.ui_size_info(),
            self.window.scale_factor as f32,
            self.sidebar_model(),
            self.left_sidebar_progress(),
        );
        let (bar, max, current) = match drag.kind {
            chrome::SidebarScrollKind::Tabs => {
                (layout.tabs_scrollbar, layout.tabs_max_scroll, self.nebula_tabs_scroll)
            },
            chrome::SidebarScrollKind::Hosts => {
                (layout.hosts_scrollbar, layout.hosts_max_scroll, self.nebula_hosts_scroll)
            },
        };
        let Some(bar) = bar else { return false };
        let target = bar.target_offset(y, drag.grab, max);
        if target == current {
            return false;
        }
        match drag.kind {
            chrome::SidebarScrollKind::Tabs => self.nebula_tabs_scroll = target,
            chrome::SidebarScrollKind::Hosts => self.nebula_hosts_scroll = target,
        }
        self.pending_update.dirty = true;
        true
    }

    pub fn sidebar_scrollbar_dragging(&self) -> bool {
        self.nebula_sidebar_scroll_drag.is_some()
    }

    pub fn end_sidebar_scrollbar_drag(&mut self) -> bool {
        self.nebula_sidebar_scroll_drag.take().is_some()
    }
}
