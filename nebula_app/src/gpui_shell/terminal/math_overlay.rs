//! GPUI 壳终端的公式覆盖层。
//!
//! 探测/持久化/fit/几何全部复用旧壳 `display::terminal_math`（单一权威，
//! 两壳像素级同源）；本文件只做三件事：在 term 锁内组织一帧的扫描与
//! 绘制计划、把覆盖掩码交给格子绘制循环跳过源格、按计划用
//! `math_view` 的位图管线上屏。
//!
//! 普通拖选保留公式与投影，选区始终指向底层源码。Vi、矩形和语义选择
//! 继续显示源码，以保留它们按源列编辑的语义。备用屏幕程序可能把光标留在静态内容末行，因此不能把
//! 那一行当作正在编辑而排除。围栏过滤与前景色走同一条
//! `scan_visible(..., rendered_cells)`
//! 合同：从 term 网格构造等价 cell 列表（`bg_alpha` = 非默认 ANSI 背景或
//! 反色），不再把空切片丢进去再另用 `bg_runs` 事后过滤。

use std::{ops::Range, sync::Arc};

use gpui::{App, Bounds, Pixels, Rgba, SharedString, Window, point, px, size};
use nebula_terminal::Term;
use nebula_terminal::event::EventListener;
use nebula_terminal::grid::Dimensions as _;
use nebula_terminal::index::{Column, Line, Point, Side};
use nebula_terminal::selection::{SelectionRange, SelectionType};
use nebula_terminal::term::TermMode;
use nebula_terminal::term::cell::Flags;
use nebula_terminal::vte::ansi::{Color, NamedColor};

use crate::display::SizeInfo;
use crate::display::color::Rgb;
use crate::display::content::RenderableCell;
use crate::display::terminal_math::{
    self, CoverageMask, FormulaOverlay, LineProjection, OverlayDrawPlan, PreparedFormula,
    TerminalMathState,
};
use crate::gpui_shell::math_view;

/// 每 pane 一份的覆盖层状态（`TerminalView` 持有）。内部的持久锚点/缓存
/// 语义（滚动、reflow、清屏失效）全部由共享的 [`TerminalMathState`] 决定。
#[derive(Default)]
pub struct MathOverlay {
    state: TerminalMathState,
    resources_bound: bool,
    pub(super) molecules: super::molecule_overlay::MoleculeOverlay,
}

/// 一条通过几何计划的公式：源码 + 绘制计划（坐标系 = 网格局部逻辑 px）。
pub struct PlannedFormula {
    source: Arc<str>,
    plan: OverlayDrawPlan,
    /// 本条公式盖住的源格跨度 `(行, 起, 止)`：位图预检剔除公式之后要按存活
    /// 的公式重建掩码。
    spans: Vec<(usize, usize, usize)>,
    selected: bool,
}

/// 扫描/fit 已通过、等待位图预检的公式。预检只依赖公式内容、字号和颜色，
/// 不依赖最终屏幕原点；因此先在空投影下生成候选，过滤完成后再统一重算位置。
struct FormulaCandidate {
    overlay: FormulaOverlay,
    prepared: PreparedFormula,
    plan: OverlayDrawPlan,
}

/// term 锁内产生、锁外消费的候选帧。`MathAssets` 不能在 term 锁内访问，
/// 否则字体/位图缓存的工作会把 PTY 网格锁拖进渲染慢路径。
pub struct PendingMathFrame {
    candidates: Vec<FormulaCandidate>,
    size: SizeInfo,
    pixels_per_point: f32,
    reflow_inline: bool,
    selection: Option<SelectionRange>,
    viewport_origin: Line,
}

impl PendingMathFrame {
    fn empty(size: SizeInfo, pixels_per_point: f32) -> Self {
        Self {
            candidates: Vec::new(),
            size,
            pixels_per_point,
            reflow_inline: false,
            selection: None,
            viewport_origin: Line(0),
        }
    }
}

/// 一帧的覆盖产物：绘制清单 + 源格跳过掩码。
#[derive(Default)]
pub struct MathFrame {
    formulas: Vec<PlannedFormula>,
    coverage: CoverageMask,
    projection: LineProjection,
}

impl MathFrame {
    /// 该格是否被某条公式覆盖（覆盖格不画原文）。
    pub fn covers(&self, row: usize, column: usize) -> bool {
        !self.coverage.is_empty() && self.coverage.covers(Point::new(row, Column(column)))
    }

    pub fn is_empty(&self) -> bool {
        self.formulas.is_empty()
    }

    /// 普通格的源列映射到这一帧的视觉列；公式源格以及越过右边界的格返回
    /// `None`。调用方仍以源坐标决定颜色、链接和光标语义。
    pub fn project_cell(&self, row: usize, source_column: usize, columns: usize) -> Option<usize> {
        self.projection
            .project_cell(Point::new(row, Column(source_column)), columns)
            .map(|point| point.column.0)
    }

    pub(super) fn project_image_runs(
        &self,
        runs: &[nebula_terminal::inline_image::ImageRun],
        columns: usize,
    ) -> Vec<nebula_terminal::inline_image::ImageRun> {
        self.projection.project_image_runs(runs, columns)
    }

    /// 公式源码格的 ANSI 背景映射。视觉公式盒之外的多余源码格会被丢弃，
    /// 避免压缩后仍留下一条源宽度的背景色带。
    pub fn project_formula_background(
        &self,
        row: usize,
        source_column: usize,
        columns: usize,
    ) -> Option<usize> {
        self.projection
            .project_formula_background(Point::new(row, Column(source_column)), columns)
            .map(|point| point.column.0)
    }

    /// 光标/浮层锚点优先按普通格投影；极端情况下锚点落进公式源区，则退到
    /// 公式视觉背景能承载的位置。活动编辑行本来就不投影，这个分支主要防御
    /// 历史链接锚点。
    pub fn visual_column(&self, row: usize, source_column: usize, columns: usize) -> usize {
        self.project_cell(row, source_column, columns)
            .or_else(|| self.project_formula_background(row, source_column, columns))
            .unwrap_or_else(|| source_column.min(columns.saturating_sub(1)))
    }

    /// 把源 run 投影成若干连续视觉 run。背景跨过公式时必须逐源格映射，
    /// 但最终仍合并相邻视觉格，避免给每个背景 cell 单独提交一个 quad。
    pub fn projected_runs(
        &self,
        row: usize,
        source: Range<usize>,
        columns: usize,
        formula_background: bool,
    ) -> Vec<Range<usize>> {
        let mut runs = Vec::new();
        let mut current: Option<Range<usize>> = None;
        for source_column in source {
            let target = if formula_background && self.covers(row, source_column) {
                self.project_formula_background(row, source_column, columns)
            } else {
                self.project_cell(row, source_column, columns)
            };
            match target {
                Some(column) if current.as_ref().is_some_and(|run| run.end == column) => {
                    current.as_mut().expect("current visual run").end += 1;
                },
                Some(column) => {
                    if let Some(run) = current.take() {
                        runs.push(run);
                    }
                    current = Some(column..column + 1);
                },
                None => {
                    if let Some(run) = current.take() {
                        runs.push(run);
                    }
                },
            }
        }
        if let Some(run) = current {
            runs.push(run);
        }
        runs
    }
}

/// 计划里的前景色（`paint_frame` 与位图预检必须用同一个值，否则缓存键不同、
/// 预检的那张图白合成一遍）。
fn plan_color(plan: &OverlayDrawPlan) -> Rgba {
    Rgba {
        r: plan.foreground.r as f32 / 255.0,
        g: plan.foreground.g as f32 / 255.0,
        b: plan.foreground.b as f32 / 255.0,
        a: 1.0,
    }
}

impl MathOverlay {
    /// session 暂不可用时也要立即清掉上一帧投影；输入命中发生在 paint 之外，
    /// 不能等下一次有 term 的扫描再修正旧坐标。
    pub(super) fn bind_resources(&mut self, cx: &App) {
        if self.resources_bound {
            return;
        }
        let engine = super::super::scientific_render::assets(cx);
        self.state.set_layout_resolver(Arc::new(move |source, size, points, display| {
            engine.layout(super::super::scientific_render::FormulaKey::new(
                source.into(),
                display,
                false,
                size,
                points,
            ))
        }));
        self.resources_bound = true;
    }

    pub fn clear_frame(&mut self, size: &SizeInfo, pixels_per_point: f32) -> PendingMathFrame {
        self.state.update_projection(&[], &[], false);
        PendingMathFrame::empty(*size, pixels_per_point)
    }

    /// 每帧的扫描 + 预编 + 几何计划。必须在 term 锁内调用。
    ///
    /// `size` 以网格左上角为原点（padding = 0），行列必须与可见网格一致，
    /// 这样 `TextGrid::from_term` 的 history lookback 仍按视口顶锚定。
    pub fn plan_frame<T: EventListener>(
        &mut self,
        term: &Term<T>,
        size: &SizeInfo,
        default_foreground: Rgb,
        font_pixel_size: f32,
        pixels_per_point: f32,
        include: impl Fn(usize) -> bool,
    ) -> PendingMathFrame {
        let alt_screen = term.mode().contains(TermMode::ALT_SCREEN);
        let selection = term.selection.as_ref().and_then(|selection| selection.to_range(term));
        // Rectangular/semantic selection depends on source-column expansion.
        // Ordinary drags already use the shared formula boundary hit mapping;
        // clearing that mapping mid-drag would move the text under the pointer.
        if term.mode().intersects(TermMode::VI)
            || (selection.is_some()
                && term.selection.as_ref().is_some_and(|selection| {
                    matches!(selection.ty, SelectionType::Block | SelectionType::Semantic)
                }))
        {
            return self.clear_frame(size, pixels_per_point);
        }

        let origin = term.viewport_origin_for(size.screen_lines());
        // 光标所在行是活动输入，必须保留原文。备用屏幕里同样放过：编辑器
        // 的光标压在你要改的那一行上，换成渲染图就没法编辑源码了。
        let cursor =
            nebula_terminal::term::point_to_viewport_from(origin, term.grid().cursor.point).filter(
                |point| point.line < size.screen_lines() && point.column.0 < size.columns(),
            );
        let rendered_cells = scan_cells_from_term(term, size, default_foreground);
        let overlays = terminal_math::scan_visible(
            &mut self.state,
            term,
            size,
            &rendered_cells,
            alt_screen,
            cursor,
            default_foreground,
            include,
        );

        let prepared = terminal_math::prepare_overlays(
            &mut self.state,
            &overlays,
            size,
            font_pixel_size,
            pixels_per_point,
        );
        // 候选计划必须在空投影下生成。位图预检会淘汰无法合成的公式；若先把
        // 它们计入累计 shift，后续存活公式的原点就会沿用一个不存在的压缩量。
        self.state.update_projection(&[], &[], false);
        let candidates = overlays
            .into_iter()
            .zip(prepared)
            .filter_map(|(overlay, prepared)| {
                let prepared = prepared?;
                let plan = terminal_math::plan_overlay_draw(
                    &mut self.state,
                    &overlay,
                    &prepared,
                    size,
                    pixels_per_point,
                )?;
                Some(FormulaCandidate { overlay, prepared, plan })
            })
            .collect();
        PendingMathFrame {
            candidates,
            size: *size,
            pixels_per_point,
            // 备用屏同样重排：边框、独立列和着色表面由 `reflow_limit`
            // 固定在原列，只有公式后面的正文跟着压缩。
            reflow_inline: true,
            selection,
            viewport_origin: origin,
        }
    }

    /// 位图预检并完成一帧。必须在 term 锁外调用；成功图像会在这里进入缓存，
    /// [`paint_frame`] 随后消费相同键，不会重复合成。
    pub fn finalize_frame(
        &mut self,
        pending: PendingMathFrame,
        raster_scale: f32,
        cx: &mut App,
    ) -> MathFrame {
        let can_rasterize = cx
            .try_global::<math_view::MathAssets>()
            .is_some_and(math_view::MathAssets::can_rasterize);
        let pixels_per_point = pending.pixels_per_point;
        self.finalize_frame_with(pending, |source, plan| {
            if !can_rasterize {
                return false;
            }
            let source: SharedString = source.to_string().into();
            cx.global_mut::<math_view::MathAssets>().can_compose(
                &source,
                plan.display_style,
                plan.fitted_pixel_size,
                pixels_per_point,
                raster_scale,
                plan_color(plan),
            )
        })
    }

    /// 预检策略与几何收敛拆开，测试可以用确定的存活集合验证 projection，
    /// 不需要启动 GPUI 字体后端。
    fn finalize_frame_with(
        &mut self,
        mut pending: PendingMathFrame,
        mut paintable: impl FnMut(&Arc<str>, &OverlayDrawPlan) -> bool,
    ) -> MathFrame {
        pending.candidates.retain(|candidate| {
            let source = candidate.overlay.source_arc();
            paintable(&source, &candidate.plan)
        });

        // plan 也可能在最终横向 shift 后退化。每淘汰一条就重新建立共享
        // LineProjection，直到 projection、coverage、draw plan 使用同一集合。
        loop {
            let overlays: Vec<FormulaOverlay> =
                pending.candidates.iter().map(|candidate| candidate.overlay.clone()).collect();
            let prepared: Vec<Option<PreparedFormula>> =
                pending.candidates.iter().map(|candidate| Some(candidate.prepared)).collect();
            let retained = self.state.update_projection_with_survivors(
                &overlays,
                &prepared,
                pending.reflow_inline,
            );

            if retained.iter().any(|keep| !keep) {
                let mut index = 0usize;
                pending.candidates.retain(|_| {
                    let keep = retained[index];
                    index += 1;
                    keep
                });
                continue;
            }

            let before = pending.candidates.len();
            pending.candidates.retain_mut(|candidate| {
                let Some(plan) = terminal_math::plan_overlay_draw(
                    &mut self.state,
                    &candidate.overlay,
                    &candidate.prepared,
                    &pending.size,
                    pending.pixels_per_point,
                ) else {
                    return false;
                };
                candidate.plan = plan;
                true
            });
            if pending.candidates.len() == before {
                break;
            }
        }

        let formulas: Vec<PlannedFormula> = pending
            .candidates
            .into_iter()
            .map(|candidate| PlannedFormula {
                source: candidate.overlay.source_arc(),
                plan: candidate.plan,
                spans: candidate.overlay.covered_spans().collect(),
                selected: pending.selection.is_some_and(|selection| {
                    candidate.overlay.covered_spans().any(|(row, start, end)| {
                        let line = pending.viewport_origin + row;
                        line >= selection.start.line
                            && line <= selection.end.line
                            && (start..end)
                                .any(|column| selection.contains(Point::new(line, Column(column))))
                    })
                }),
            })
            .collect();
        let spans: Vec<(usize, usize, usize)> =
            formulas.iter().flat_map(|formula| formula.spans.iter().copied()).collect();
        MathFrame {
            coverage: CoverageMask::from_spans(spans),
            projection: self.state.projection_snapshot(),
            formulas,
        }
    }

    /// GPUI 所有 terminal-cell 输入的反投影入口。调用方传入与当前 term 锁
    /// 同源的 viewport origin，避免滚动或 resize commit 时把视觉行映错。
    pub fn source_point(&self, point: Point, side: Side, viewport_origin: Line) -> (Point, Side) {
        self.state.source_point(point, side, viewport_origin)
    }
}

/// 可见网格的 SizeInfo：padding = 0，行列与元素布局一致。
///
/// 半格余量是为了 `SizeInfo::assemble` 里 `(width / cell_width) as usize`
/// 不会把 `8.399994 * 120 / 8.399994 = 119.999` 截成少一列，从而丢掉行尾
/// 的 `\]` / `\)`。
pub(crate) fn grid_size_info(
    columns: usize,
    rows: usize,
    cell_width: f32,
    cell_height: f32,
) -> SizeInfo {
    SizeInfo::new(
        cell_width * columns as f32 + cell_width * 0.5,
        cell_height * rows as f32 + cell_height * 0.5,
        cell_width,
        cell_height,
        0.0,
        0.0,
        false,
    )
}

/// 旧壳 `RenderableContent` 迭代器的 GPUI 等价物：给 `scan_visible` 填
/// fallback / `bg_alpha` / 前景。`bg_alpha` 对齐 `compute_bg_alpha`（无
/// transparent_background_colors）：默认 Named 背景为 0，反色或其它 ANSI
/// 背景为 1。空格+默认背景跳过，与旧壳 `is_empty` 一致。
fn scan_cells_from_term<T: EventListener>(
    term: &Term<T>,
    size: &SizeInfo,
    default_foreground: Rgb,
) -> Vec<RenderableCell> {
    let grid = term.grid();
    let origin = term.viewport_origin_for(size.screen_lines());
    let columns = size.columns().min(grid.columns());
    let screen_lines = size
        .screen_lines()
        .min(((grid.bottommost_line() - origin).0.max(0) as usize).saturating_add(1));
    let mut cells = Vec::new();
    for row in 0..screen_lines {
        let line = origin + row as i32;
        for column in 0..columns {
            let cell = &grid[line][Column(column)];
            if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                continue;
            }
            let bg_alpha = if cell.flags.contains(Flags::INVERSE) {
                1.0
            } else if cell.bg == Color::Named(NamedColor::Background) {
                0.0
            } else {
                1.0
            };
            if bg_alpha == 0.0
                && cell.c == ' '
                && !cell.flags.intersects(Flags::ALL_UNDERLINES | Flags::STRIKEOUT)
            {
                continue;
            }
            let fg = cell_foreground(cell, default_foreground);
            cells.push(RenderableCell {
                character: cell.c,
                point: Point::new(row, Column(column)),
                fg,
                bg: Rgb::default(),
                bg_alpha,
                underline: fg,
                flags: cell.flags,
                extra: None,
            });
        }
    }
    cells
}

fn cell_foreground(cell: &nebula_terminal::term::cell::Cell, default: Rgb) -> Rgb {
    let color = if cell.flags.contains(Flags::INVERSE) { cell.bg } else { cell.fg };
    match color {
        Color::Spec(rgb) => Rgb::new(rgb.r, rgb.g, rgb.b),
        _ => default,
    }
}

/// 按计划绘制一帧的公式（在格子文本之后调用）。`origin` 是网格左上角的
/// 窗口坐标；`cell` 是 (宽, 高)，text ops 的"整字符可见才画"合同用它。
pub fn paint_frame(
    frame: &MathFrame,
    origin: gpui::Point<Pixels>,
    cell: (f32, f32),
    pixels_per_point: f32,
    selection_color: Rgba,
    map_y: &dyn Fn(f32) -> f32,
    window: &mut Window,
    cx: &mut App,
) {
    for formula in &frame.formulas {
        let plan = &formula.plan;
        let color = plan_color(plan);
        let source: SharedString = formula.source.to_string().into();

        // 裁剪跟随 bleed 预算（与旧壳 MathClip 同一矩形），交给 GPUI 的
        // content mask 执行像素级裁剪。
        let clip = Bounds::new(
            point(origin.x + px(plan.clip_left), origin.y + px(map_y(plan.clip_top))),
            size(
                px((plan.clip_right - plan.clip_left).max(0.0)),
                px((map_y(plan.clip_bottom) - map_y(plan.clip_top)).max(0.0)),
            ),
        );
        let left = origin.x + px(plan.origin_x);
        let baseline = origin.y + px(map_y(plan.baseline_y));
        window.with_content_mask(Some(gpui::ContentMask { bounds: clip }), |window| {
            math_view::paint_formula_image(
                &source,
                plan.display_style,
                plan.fitted_pixel_size,
                pixels_per_point,
                left,
                baseline,
                color,
                window,
                cx,
            );

            // 数学字体缺字的字符（公式里的中文等）：旧壳合同是整字符落在
            // 裁剪窗内才画，避免半个汉字。
            let layout = cx.global_mut::<math_view::MathAssets>().layout(
                &source,
                plan.display_style,
                plan.fitted_pixel_size,
                pixels_per_point,
            );
            let Some(layout) = layout else { return };
            let text_style = window.text_style();
            for op in &layout.text {
                let scale = op.pixel_size / plan.fitted_pixel_size.max(f32::EPSILON);
                let x = plan.origin_x + op.x;
                let width = cell.0 * scale;
                let height = cell.1 * scale;
                let top = plan.baseline_y + op.baseline_y - height * 0.8;
                if x < plan.clip_left
                    || x + width > plan.clip_right
                    || top < plan.clip_top
                    || top + height > plan.clip_bottom
                {
                    continue;
                }
                let mut buffer = [0u8; 4];
                let text: SharedString = op.character.encode_utf8(&mut buffer).to_string().into();
                let run = text_style.to_run(text.len());
                let line = window.text_system().shape_line(
                    text,
                    px(op.pixel_size),
                    std::slice::from_ref(&run),
                    None,
                );
                let target = point(
                    origin.x + px(x),
                    origin.y + px(map_y(plan.baseline_y + op.baseline_y)) - line.ascent,
                );
                let _ = line.paint(
                    target,
                    line.ascent + line.descent,
                    gpui::TextAlign::Left,
                    None,
                    window,
                    cx,
                );
            }
        });
        if formula.selected {
            // Outline the selected formula as an object. Its existing bitmap
            // and foreground stay intact, without a new raster/color cache key.
            window.paint_quad(gpui::outline(clip, selection_color, gpui::BorderStyle::Solid));
        }
    }
}

#[cfg(test)]
mod tests {
    use nebula_terminal::event::VoidListener;
    use nebula_terminal::grid::Dimensions;
    use nebula_terminal::index::{Column, Line, Point};
    use nebula_terminal::selection::{Selection, SelectionType};
    use nebula_terminal::term::Config;
    use nebula_terminal::vte::ansi::Color;
    use nebula_terminal::{Term, index::Side};

    use super::{MathOverlay, grid_size_info, scan_cells_from_term};
    use crate::display::color::Rgb;

    struct TestSize {
        cols: usize,
        rows: usize,
    }

    impl Dimensions for TestSize {
        fn total_lines(&self) -> usize {
            self.rows
        }

        fn screen_lines(&self) -> usize {
            self.rows
        }

        fn columns(&self) -> usize {
            self.cols
        }
    }

    fn term_with(cols: usize, rows: usize, lines: &[&str]) -> Term<VoidListener> {
        let mut term = Term::new(Config::default(), &TestSize { cols, rows }, VoidListener);
        for (row, text) in lines.iter().enumerate() {
            let mut col = 0usize;
            for ch in text.chars() {
                if col >= cols {
                    break;
                }
                let cell = &mut term.grid_mut()[Line(row as i32)][Column(col)];
                cell.c = ch;
                col += 1;
            }
        }
        // 公式行不能与光标逻辑行重合，否则 scan_visible 会整段排除。
        term.grid_mut().cursor.point = Point::new(Line(rows as i32 - 1), Column(0));
        term
    }

    fn plan(
        overlay: &mut MathOverlay,
        term: &Term<VoidListener>,
        cols: usize,
        rows: usize,
    ) -> super::MathFrame {
        let size = grid_size_info(cols, rows, 8.0, 16.0);
        let pending =
            overlay.plan_frame(term, &size, Rgb::new(0xd6, 0xda, 0xea), 16.0, 1.0, |_| true);
        overlay.finalize_frame_with(pending, |_, _| true)
    }

    #[test]
    fn native_source_filter_excludes_hidden_formulas_and_keeps_visible_tail() {
        let term = term_with(24, 8, &[r"$x_1$", "", r"$y_2$", "", r"$z_3$"]);
        let size = grid_size_info(24, 8, 8.0, 16.0);
        let mut overlay = MathOverlay::default();
        let pending =
            overlay.plan_frame(&term, &size, Rgb::new(0xd6, 0xda, 0xea), 16.0, 1.0, |row| row != 2);
        let frame = overlay.finalize_frame_with(pending, |_, _| true);
        assert_eq!(frame.formulas.len(), 2);
        assert!(frame.covers(0, 0));
        assert!(!frame.covers(2, 0));
        assert!(frame.covers(4, 0));
    }

    #[test]
    fn hidden_native_formulas_do_not_exhaust_visible_scan_budget() {
        let hidden = "$x$ ".repeat(64);
        let term = term_with(320, 8, &[&hidden, "", "", "", "$z_3$"]);
        let size = grid_size_info(320, 8, 8.0, 16.0);
        let mut overlay = MathOverlay::default();
        assert_eq!(plan(&mut overlay, &term, 320, 8).formulas.len(), 64);
        for _ in 0..2 {
            let pending =
                overlay.plan_frame(&term, &size, Rgb::new(0xd6, 0xda, 0xea), 16.0, 1.0, |row| {
                    row != 0
                });
            let frame = overlay.finalize_frame_with(pending, |_, _| true);
            assert_eq!(frame.formulas.len(), 1);
            assert!(frame.covers(4, 0));
        }
    }

    #[test]
    fn adjacent_image_rows_are_solid_formula_clip_boundaries() {
        use nebula_terminal::inline_image::ImageLayout;
        for prefixed in [false, true] {
            let prefix = if prefixed { format!("$x{}$ ", " ".repeat(60)) } else { String::new() };
            let text = format!("{prefix}{}", r"$\frac{1}{\frac{x}{y}}$");
            let mut term = term_with(120, 6, &["", "", &text]);
            let mut overlay = MathOverlay::default();
            overlay.state.set_layout_resolver(std::sync::Arc::new(|source, pixels, _, _| {
                let scale = pixels / 16.0;
                let mut layout = crate::math::layout::MathLayout::default();
                layout.metrics.width = if source.as_ref() == "x" { 8.0 } else { 16.0 } * scale;
                layout.metrics.height = if source.as_ref() == "x" { 12.0 } else { 24.0 } * scale;
                layout.metrics.depth = if source.as_ref() == "x" { 0.0 } else { 8.0 } * scale;
                Some(std::sync::Arc::new(layout))
            }));
            let before = plan(&mut overlay, &term, 120, 6);
            assert_eq!(before.formulas.len(), if prefixed { 2 } else { 1 });
            assert!(before.formulas.last().unwrap().plan.clip_top < 32.0);
            for row in [1, 3] {
                term.grid_mut().cursor.point =
                    Point::new(Line(row), Column(if prefixed { 3 } else { 0 }));
                term.nebula_insert_inline_image(ImageLayout {
                    width: 72.0,
                    height: 16.0,
                    cell_width: 8.0,
                    cell_height: 16.0,
                    columns: 9,
                    rows: 1,
                });
            }
            term.grid_mut().cursor.point = Point::new(Line(5), Column(0));
            let frame = plan(&mut overlay, &term, 120, 6);
            assert_eq!(frame.formulas.len(), if prefixed { 2 } else { 1 });
            let tall = &frame.formulas.last().unwrap().plan;
            assert!(tall.clip_top >= 32.0, "prefixed={prefixed}, clip_top={}", tall.clip_top);
            assert!(
                tall.clip_bottom <= 48.0,
                "prefixed={prefixed}, clip_bottom={}",
                tall.clip_bottom
            );
        }
    }

    #[test]
    fn grid_size_info_does_not_drop_a_column_to_float_truncation() {
        let cell_w = 8.399994;
        let size = grid_size_info(120, 30, cell_w, 16.8);
        assert_eq!(size.columns(), 120);
        assert_eq!(size.screen_lines(), 30);
        assert_eq!(size.padding_x(), 0.0);
        assert_eq!(size.padding_y(), 0.0);
    }

    #[test]
    fn scan_cells_keep_backslash_and_skip_default_background_spaces() {
        let term = term_with(16, 3, &[r"\[x\]", "tail"]);
        let size = grid_size_info(16, 3, 8.0, 16.0);
        let cells = scan_cells_from_term(&term, &size, Rgb::new(255, 255, 255));
        let row0: String =
            cells.iter().filter(|cell| cell.point.line == 0).map(|cell| cell.character).collect();
        assert_eq!(row0, r"\[x\]");
        assert!(cells.iter().all(|cell| cell.bg_alpha == 0.0));
    }

    #[test]
    fn bracket_display_scans_without_inline_dollar() {
        let mut overlay = MathOverlay::default();
        let term = term_with(16, 4, &[r"\[x\]", "", "prompt"]);
        let frame = plan(&mut overlay, &term, 16, 4);
        assert!(!frame.is_empty(), "\\[x\\] is display math and must overlay without AI CLI");
        assert!(frame.covers(0, 0));
        assert!(frame.covers(0, 4), "closing ] must be covered");
        assert_eq!(frame.formulas[0].source.as_ref(), "x");
        assert!(frame.formulas[0].plan.display_style, "\\[ must typeset in display style");
    }

    #[test]
    fn parenthesized_inline_scans_without_ai_cli() {
        let mut overlay = MathOverlay::default();
        let term = term_with(16, 4, &[r"\(x\)", "", "prompt"]);
        let frame = plan(&mut overlay, &term, 16, 4);
        assert!(!frame.is_empty(), "\\(x\\) is standard inline math");
        assert!(frame.covers(0, 0));
        assert_eq!(frame.formulas[0].source.as_ref(), "x");
        assert!(!frame.formulas[0].plan.display_style);
    }

    #[test]
    fn inline_dollar_uses_the_standard_formula_rule() {
        let mut overlay = MathOverlay::default();
        let term = term_with(16, 4, &["$x$", "", "prompt"]);
        let frame = plan(&mut overlay, &term, 16, 4);
        assert!(!frame.is_empty(), "$x$ is standard inline math without AI context");
        assert_eq!(frame.formulas[0].source.as_ref(), "x");
        assert!(!frame.formulas[0].plan.display_style);
    }

    #[test]
    fn main_screen_compacts_inline_suffix_without_changing_source_columns() {
        let mut overlay = MathOverlay::default();
        let term = term_with(20, 4, &["$x$ suffix", "", "prompt"]);
        let frame = plan(&mut overlay, &term, 20, 4);
        let source_suffix = 3;
        let visual_suffix =
            frame.project_cell(0, source_suffix, 20).expect("suffix remains inside the viewport");
        assert!(visual_suffix < source_suffix, "inline suffix must move into the compact gap");
        assert!(frame.covers(0, 0));
        assert!(frame.covers(0, 2));
        assert_eq!(term.grid()[Line(0)][Column(source_suffix)].c, ' ');
    }

    #[test]
    fn failed_formula_does_not_shift_later_survivors() {
        let line = "a $x$ b $y^2$ c";
        let term = term_with(32, 4, &[line, "", "prompt"]);
        let size = grid_size_info(32, 4, 8.0, 16.0);

        let mut all_overlay = MathOverlay::default();
        let all_pending =
            all_overlay.plan_frame(&term, &size, Rgb::new(0xd6, 0xda, 0xea), 16.0, 1.0, |_| true);
        let all = all_overlay.finalize_frame_with(all_pending, |_, _| true);

        let mut filtered_overlay = MathOverlay::default();
        let filtered_pending = filtered_overlay.plan_frame(
            &term,
            &size,
            Rgb::new(0xd6, 0xda, 0xea),
            16.0,
            1.0,
            |_| true,
        );
        let filtered = filtered_overlay
            .finalize_frame_with(filtered_pending, |source, _| source.as_ref() != "x");

        assert!(!filtered.covers(0, 2), "failed first formula must leave its source visible");
        assert!(filtered.covers(0, 8), "later paintable formula still overlays");
        assert_eq!(filtered.formulas.len(), 1);
        assert_eq!(
            filtered.project_formula_background(0, 8, 32),
            Some(8),
            "the surviving formula must not inherit the failed formula's shift"
        );
        let source_suffix = 13;
        let filtered_suffix =
            filtered.project_cell(0, source_suffix, 32).expect("filtered suffix remains visible");
        let all_suffix =
            all.project_cell(0, source_suffix, 32).expect("unfiltered suffix remains visible");
        assert!(
            filtered_suffix > all_suffix,
            "the failed formula must not contribute a stale cumulative shift"
        );
    }

    #[test]
    fn ansi_background_keeps_formula_source_literal() {
        let mut overlay = MathOverlay::default();
        let mut term = term_with(16, 4, &["$x$", "", "prompt"]);
        for col in 0..3 {
            term.grid_mut()[Line(0)][Column(col)].bg = Color::Indexed(4);
        }
        let frame = plan(&mut overlay, &term, 16, 4);
        assert!(frame.is_empty(), "TUI- or code-owned backgrounds stay untouched");
    }

    #[test]
    fn markdown_stripped_bare_delimiter_still_needs_ansi_context() {
        let mut overlay = MathOverlay::default();
        let mut term = term_with(24, 4, &[r"(\sqrt{x})", "", "prompt"]);
        for col in 0..10 {
            term.grid_mut()[Line(0)][Column(col)].bg = Color::Indexed(4);
        }
        let frame = plan(&mut overlay, &term, 24, 4);
        assert!(frame.is_empty(), "bare parentheses remain ambiguous on an ANSI code surface");
    }

    #[test]
    fn empty_selection_does_not_disable_overlays() {
        let mut overlay = MathOverlay::default();
        let mut term = term_with(16, 4, &[r"\[x\]", "", "prompt"]);
        term.selection =
            Some(Selection::new(SelectionType::Simple, Point::new(Line(2), Column(0)), Side::Left));
        let frame = plan(&mut overlay, &term, 16, 4);
        assert!(!frame.is_empty(), "zero-width leftover selection must not gate the frame");
    }

    #[test]
    fn ordinary_selection_preserves_projection_and_copies_the_formula_source() {
        let mut overlay = MathOverlay::default();
        let mut term = term_with(20, 4, &["$x^2$ suffix", "", "prompt"]);
        let source_suffix = 5;
        let projected = plan(&mut overlay, &term, 20, 4);
        assert!(projected.project_cell(0, source_suffix, 20).unwrap() < source_suffix);

        let visual_suffix = projected.project_cell(0, source_suffix, 20).unwrap();
        let (start, start_side) =
            overlay.source_point(Point::new(Line(0), Column(0)), Side::Left, Line(0));
        let (end, end_side) = overlay.source_point(
            Point::new(Line(0), Column(visual_suffix - 1)),
            Side::Right,
            Line(0),
        );
        let mut selection = Selection::new(SelectionType::Simple, start, start_side);
        selection.update(end, end_side);
        term.selection = Some(selection);
        let selected = plan(&mut overlay, &term, 20, 4);

        assert_eq!(term.selection_to_string().as_deref(), Some("$x^2$"));
        assert!(!selected.is_empty());
        assert_eq!(selected.project_cell(0, source_suffix, 20), Some(visual_suffix));
        assert!(selected.formulas[0].selected);
        assert_eq!(selected.formulas[0].source, projected.formulas[0].source);
        assert_eq!(selected.formulas[0].plan.foreground, projected.formulas[0].plan.foreground);
        assert_eq!(
            selected.formulas[0].plan.fitted_pixel_size,
            projected.formulas[0].plan.fitted_pixel_size
        );
    }

    #[test]
    fn source_column_selection_modes_keep_their_existing_source_view() {
        for kind in [SelectionType::Block, SelectionType::Semantic] {
            let mut overlay = MathOverlay::default();
            let mut term = term_with(20, 4, &["$x^2$ suffix", "", "prompt"]);
            assert!(!plan(&mut overlay, &term, 20, 4).is_empty());
            let mut selection = Selection::new(kind, Point::new(Line(0), Column(0)), Side::Left);
            selection.update(Point::new(Line(0), Column(2)), Side::Right);
            term.selection = Some(selection);
            let frame = plan(&mut overlay, &term, 20, 4);
            assert!(frame.is_empty());
            assert_eq!(frame.project_cell(0, 5, 20), Some(5));
        }
    }

    #[test]
    fn selecting_another_row_does_not_move_or_mark_a_formula() {
        let mut overlay = MathOverlay::default();
        let mut term = term_with(32, 4, &["$x^2$ suffix", "other text", "prompt"]);
        let before = plan(&mut overlay, &term, 32, 4);
        let mut selection =
            Selection::new(SelectionType::Simple, Point::new(Line(1), Column(0)), Side::Left);
        selection.update(Point::new(Line(1), Column(4)), Side::Right);
        term.selection = Some(selection);
        let after = plan(&mut overlay, &term, 32, 4);
        assert!(!after.is_empty());
        assert_eq!(after.project_cell(0, 5, 32), before.project_cell(0, 5, 32));
        assert!(!after.formulas[0].selected);
    }

    #[test]
    fn issue_119_inline_formula_bodies_produce_render_plans() {
        for source in [
            r"$\sigma(\mathbf{z} + c) = \sigma(\mathbf{z})$",
            r"$\dfrac{\partial \sigma_i}{\partial z_j} = \sigma_i\delta_{ij} - \sigma_i\sigma_j$",
        ] {
            let mut overlay = MathOverlay::default();
            let term = term_with(160, 4, &[source, "", "prompt"]);
            let frame = plan(&mut overlay, &term, 160, 4);
            assert_eq!(frame.formulas.len(), 1, "{source}");
            assert_eq!(frame.formulas[0].source.as_ref(), &source[1..source.len() - 1]);
        }
    }

    #[test]
    fn explicit_empty_frame_clears_existing_projection() {
        let mut overlay = MathOverlay::default();
        let term = term_with(20, 4, &["$x^2$ suffix", "", "prompt"]);
        let source_suffix = 5;
        let projected = plan(&mut overlay, &term, 20, 4);
        assert!(projected.project_cell(0, source_suffix, 20).unwrap() < source_suffix);

        let size = grid_size_info(20, 4, 8.0, 16.0);
        let pending = overlay.clear_frame(&size, 1.0);
        let cleared = overlay.finalize_frame_with(pending, |_, _| true);

        assert!(cleared.is_empty());
        assert_eq!(cleared.project_cell(0, source_suffix, 20), Some(source_suffix));
    }

    #[test]
    fn alt_screen_compacts_agent_prose_but_keeps_tui_columns() {
        let formula = r"$n! \approx \sqrt{2\pi n}\left(\dfrac{n}{e}\right)^n$";
        let line = format!("{formula} tail   sidebar │ edge");
        let column_of = |needle: &str| line[..line.find(needle).unwrap()].chars().count();
        let (tail, sidebar, border) = (column_of("tail"), column_of("sidebar"), column_of("│"));
        let mut overlay = MathOverlay::default();
        let mut term = term_with(96, 4, &[&line, "", "prompt"]);
        let main_frame = plan(&mut overlay, &term, 96, 4);
        assert!(main_frame.project_cell(0, tail, 96).unwrap() < tail);
        term.swap_alt();
        // `swap_alt` 按终端语义先清空备用网格；测试内容必须写进交换后的活动
        // 网格，才能验证 ALT_SCREEN 的覆盖与重排合同。
        for (column, character) in line.chars().enumerate() {
            term.grid_mut()[Line(0)][Column(column)].c = character;
        }
        let frame = plan(&mut overlay, &term, 96, 4);
        assert!(!frame.is_empty(), "alternate-screen output may use the common TeX path");
        assert_eq!(
            frame.formulas[0].source.as_ref(),
            r"n! \approx \sqrt{2\pi n}\left(\dfrac{n}{e}\right)^n"
        );
        assert!(
            frame.project_cell(0, tail, 96).unwrap() < tail,
            "agent prose after an inline formula must close the gap left by the TeX source"
        );
        assert_eq!(frame.project_cell(0, sidebar, 96), Some(sidebar), "gap-aligned column");
        assert_eq!(frame.project_cell(0, border, 96), Some(border), "box-drawing border");
    }

    #[test]
    fn coloured_tui_surface_after_a_formula_keeps_its_column() {
        use nebula_terminal::event_loop::StreamProcessor;
        let mut term = Term::new(Config::default(), &TestSize { cols: 40, rows: 4 }, VoidListener);
        let mut stream = StreamProcessor::default();
        stream.feed(&mut term, &VoidListener, b"$x^2$ tail \x1b[44mband\x1b[0m\r\n\r\nprompt");
        let mut overlay = MathOverlay::default();
        let frame = plan(&mut overlay, &term, 40, 4);
        assert!(frame.project_cell(0, 6, 40).unwrap() < 6, "prose follows the formula");
        assert_eq!(frame.project_cell(0, 11, 40), Some(11), "coloured surface stays put");
    }

    #[test]
    fn markdown_inline_code_quoting_tex_stays_literal() {
        use nebula_terminal::event_loop::StreamProcessor;
        let mut term = Term::new(Config::default(), &TestSize { cols: 40, rows: 8 }, VoidListener);
        let mut stream = StreamProcessor::default();
        // Claude Code 把行内代码画成 #B1BAF9 前景、无背景（实测）。
        stream.feed(
            &mut term,
            &VoidListener,
            b"code \x1b[38;2;177;186;249m$f'(x_0)=0$\x1b[0m end\r\n\
              math $f'(x_0)=0$ end\r\n\
              \x1b[32mlog $x^2$ ok\x1b[0m\r\n\
              \x1b[32m$y^2$\x1b[0m\r\n\
              see \x1b[34mlink\x1b[0m $z^2$ \x1b[34mlink\x1b[0m\r\n\r\nprompt",
        );
        let mut overlay = MathOverlay::default();
        let frame = plan(&mut overlay, &term, 40, 8);
        let sources: Vec<_> = frame.formulas.iter().map(|f| f.source.to_string()).collect();
        assert_eq!(sources, ["f'(x_0)=0", "x^2", "y^2", "z^2"]);
        assert!(!frame.covers(0, 5), "the code span keeps its TeX source");
    }

    #[test]
    fn alt_screen_agent_answer_renders_subscripted_capitals() {
        use nebula_terminal::event_loop::StreamProcessor;
        let mut term = Term::new(Config::default(), &TestSize { cols: 40, rows: 4 }, VoidListener);
        let mut stream = StreamProcessor::default();
        // 全屏 Agent TUI：回答在上方，可见光标停在底部自绘输入框里。
        stream.feed(&mut term, &VoidListener, b"\x1b[?1049hconst $C_1$ and $C_2$ end\x1b[4;3H");
        let mut overlay = MathOverlay::default();
        let frame = plan(&mut overlay, &term, 40, 4);
        let sources: Vec<_> = frame.formulas.iter().map(|f| f.source.to_string()).collect();
        assert_eq!(sources, ["C_1", "C_2"]);
        assert!(frame.project_cell(0, 22, 40).unwrap() < 22, "answer prose closes the gap");
    }

    #[test]
    fn bracket_display_can_cross_hard_rows() {
        let mut overlay = MathOverlay::default();
        let term = term_with(16, 6, &[r"\[", "x", r"\]", "", "prompt"]);
        let frame = plan(&mut overlay, &term, 16, 6);
        assert!(!frame.is_empty(), "\\[ across hard rows must overlay");
        assert_eq!(frame.formulas[0].source.as_ref(), "x");
        assert!(frame.formulas[0].plan.display_style);
        assert!(frame.covers(0, 0));
        assert!(frame.covers(2, 0));
    }

    #[test]
    fn markdown_unescaped_bare_bracket_block_overlays() {
        // AI CLI 的 markdown 渲染吃掉 `\[` `\]` `\,` 的反斜杠后屏幕上只剩
        // 裸 `[` 块（`\int`/`\frac` 幸存）；全链路必须照常出覆盖层。
        let mut overlay = MathOverlay::default();
        let term = term_with(40, 6, &["[", r"\int_0^1 x^2,dx = \frac{1}{3}", "]", "", "prompt"]);
        let frame = plan(&mut overlay, &term, 40, 6);
        assert!(!frame.is_empty(), "markdown-unescaped bare bracket block must overlay");
        assert_eq!(frame.formulas[0].source.as_ref(), r"\int_0^1 x^2,dx = \frac{1}{3}");
        assert!(frame.formulas[0].plan.display_style);
        assert!(frame.covers(0, 0));
        assert!(frame.covers(2, 0));
    }

    #[cfg(feature = "gpui-test-support")]
    #[gpui::test]
    fn cli_screenshot_formulas_reach_async_bitmaps_without_new_terminal_output(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(crate::gpui_shell::math_view::register);
        let mut overlay = MathOverlay::default();
        cx.update(|cx| overlay.bind_resources(cx));
        let term = term_with(
            100,
            24,
            &[
                "1. quadratic",
                "",
                "  [",
                r"  x=\frac{-b\pm\sqrt{b^2-4ac}}{2a}",
                "  ]",
                "",
                "2. euler",
                "",
                "  [",
                r"  e^{i\pi}+1=0",
                "  ]",
                "",
                "3. integral",
                "",
                "  [",
                r"  \int_a^b f(x),\mathrm{d}x=F(b)-F(a)",
                "  ]",
                "",
                r"$E$ $0$ $2.71828$ $ax^2+bx+c=0$ $\pm$",
                r"$\displaystyle\int_0^1 x^2,dx",
                r"  = \dfrac{1}{3}$",
                "prompt",
            ],
        );
        let size = grid_size_info(100, 24, 8.0, 18.0);
        let mut counts = Vec::new();
        for _ in 0..12 {
            let frame = cx.update(|cx| {
                let pending =
                    overlay
                        .plan_frame(&term, &size, Rgb::new(0xec, 0xef, 0xf4), 16.0, 1.0, |_| true);
                overlay.finalize_frame(pending, 1.0, cx)
            });
            counts.push(frame.formulas.len());
            if frame.formulas.len() == 9 {
                break;
            }
            cx.run_until_parked();
        }
        assert_eq!(counts.last(), Some(&9), "async pipeline frame counts: {counts:?}");
    }

    #[test]
    fn markdown_unescaped_bare_paren_inline_overlays() {
        let mut overlay = MathOverlay::default();
        let term = term_with(40, 4, &[r"also (\sqrt{x^2+y^2}) works", "", "prompt"]);
        let frame = plan(&mut overlay, &term, 40, 4);
        assert!(!frame.is_empty(), "bare paren with whitelisted TeX command must overlay");
        assert_eq!(frame.formulas[0].source.as_ref(), r"\sqrt{x^2+y^2}");
        assert!(!frame.formulas[0].plan.display_style);
    }
    #[cfg(feature = "gpui-test-support")]
    #[gpui::test]
    fn protocol_image_with_real_math_bitmaps_keeps_text_visible(cx: &mut gpui::TestAppContext) {
        use super::super::inline_image::{InlineImageStore, decode};
        use base64::Engine as _;
        use nebula_terminal::event::{Event, EventListener, WindowSize};
        use nebula_terminal::event_loop::StreamProcessor;
        use nebula_terminal::render::{RenderSnapshot, SnapshotConfig};
        use std::io::Write as _;
        use std::sync::{Arc, Mutex};

        #[derive(Clone, Default)]
        struct Listener(Arc<Mutex<Vec<Event>>>);
        impl EventListener for Listener {
            fn send_event(&self, event: Event) {
                self.0.lock().unwrap().push(event);
            }
        }
        cx.update(crate::gpui_shell::math_view::register);
        let pixels = image::RgbaImage::from_pixel(4, 1, image::Rgba([240, 40, 40, 255]));
        let mut output = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(pixels)
            .write_to(&mut output, image::ImageFormat::Png)
            .unwrap();
        let png = base64::engine::general_purpose::STANDARD.encode(output.into_inner());
        let protocol =
            format!("\x1b]1337;File=inline=1;width=4;height=1;preserveAspectRatio=0:{png}\x07");
        let cases = [
            ("main", "", "$x^2$ ", "TAIL\r\nprompt", true),
            ("alternate", "\x1b[?1049h", "$x^2$ ", "TAIL\r\nprompt", true),
            ("active-line", "", "$x^2$ ", "TAIL", false),
            ("ansi-code-background", "", "\x1b[44m$x^2$\x1b[49m ", "TAIL\r\nprompt", false),
        ];
        let mut failures = Vec::new();
        for (label, screen, prefix, suffix, expect_formula) in cases {
            let listener = Listener::default();
            let mut term =
                Term::new(Config::default(), &TestSize { cols: 20, rows: 8 }, listener.clone());
            let mut stream = StreamProcessor::default();
            stream.resize(WindowSize {
                num_cols: 20,
                num_lines: 8,
                cell_width: 8,
                cell_height: 18,
            });
            stream.feed(
                &mut term,
                &listener,
                format!("{screen}{prefix}{protocol}{suffix}").as_bytes(),
            );
            let mut store = InlineImageStore::default();
            for event in listener.0.lock().unwrap().drain(..) {
                if let Event::InlineImage { data, placement } = event {
                    store.enqueue(data, placement).unwrap();
                }
            }
            while let Some(pending) = store.start_next() {
                store.finish(decode(pending)).unwrap();
            }
            let images = store.frame_images();
            assert_eq!(images.len(), 1, "actual production image decode: {label}");
            let snap = RenderSnapshot::capture(&term, &SnapshotConfig { rows: 8, cols: 20 });
            assert_eq!(snap.image_runs.len(), 1);
            let run = &snap.image_runs[0];
            assert_eq!((run.row, run.column, run.columns), (0, 6, 4));
            let tail: Vec<_> = snap
                .segments
                .iter()
                .filter(|segment| segment.row == 0)
                .flat_map(|segment| segment.cells.iter())
                .filter(|cell| cell.col >= 10 && cell.col < 14)
                .map(|cell| (cell.col, cell.text.clone()))
                .collect();
            assert_eq!(tail.iter().map(|(_, text)| text.as_str()).collect::<String>(), "TAIL");
            let mut overlay = MathOverlay::default();
            cx.update(|cx| overlay.bind_resources(cx));
            let size = grid_size_info(20, 8, 8.0, 18.0);
            let mut counts = Vec::new();
            let mut frame = super::MathFrame::default();
            for _ in 0..12 {
                frame = cx.update(|cx| {
                    let pending = overlay.plan_frame(
                        &term,
                        &size,
                        Rgb::new(0xec, 0xef, 0xf4),
                        16.0,
                        1.0,
                        |_| true,
                    );
                    overlay.finalize_frame(pending, 1.0, cx)
                });
                counts.push(frame.formulas.len());
                if !expect_formula || frame.formulas.len() == 1 {
                    break;
                }
                cx.run_until_parked();
            }
            assert_eq!(
                frame.formulas.len(),
                usize::from(expect_formula),
                "real bitmap pipeline {label}: {counts:?}"
            );
            let projected_images = frame.project_image_runs(&snap.image_runs, 20);
            assert_eq!(projected_images.len(), 1);
            let run = &projected_images[0];
            assert_eq!((run.source_row, run.source_column, run.columns), (0, 0, 4));
            let image_left = 8.0 * run.column as f32;
            let image_right = image_left
                + 8.0 * (images[0].placement.layout.width / images[0].placement.layout.cell_width);
            let projected: Vec<_> = tail
                .iter()
                .map(|(column, text)| {
                    let visual = frame.project_cell(0, *column as usize, 20).unwrap();
                    (text.clone(), *column, visual, 8.0 * visual as f32)
                })
                .collect();
            let overlap: Vec<_> = projected
                .iter()
                .filter(|(_, _, _, left)| *left < image_right && *left + 8.0 > image_left)
                .collect();
            writeln!(std::io::stdout().lock(), "CASE={label} formulas={} frame_counts={counts:?} image_px=[{image_left},{image_right}) tail_source_visual_px={projected:?} overlapping_cells={overlap:?}",frame.formulas.len()).unwrap();
            if !overlap.is_empty() {
                failures.push(label);
            }
        }
        assert!(
            failures.is_empty(),
            "production image paint overlaps projected text in {failures:?}"
        );
    }
    #[test]
    fn image_cells_are_not_formula_whitespace() {
        use nebula_terminal::event::WindowSize;
        use nebula_terminal::event_loop::StreamProcessor;
        let mut term = Term::new(Config::default(), &TestSize { cols: 20, rows: 8 }, VoidListener);
        let mut stream = StreamProcessor::default();
        stream.resize(WindowSize { num_cols: 20, num_lines: 8, cell_width: 8, cell_height: 18 });
        stream.feed(&mut term, &VoidListener, b"$x \x1b]1337;File=inline=1;width=4;height=1;preserveAspectRatio=0:iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLttAAAAABJRU5ErkJggg==\x07$\r\nprompt");
        assert!(term.grid()[Line(0)][Column(3)].image().is_some());
        let mut overlay = MathOverlay::default();
        assert!(plan(&mut overlay, &term, 20, 8).is_empty());
    }
    #[test]
    fn image_span_filter_only_reads_visible_rows_after_height_shrink() {
        let mut term = term_with(20, 8, &["", "$$", "x^2", "$$"]);
        term.grid_mut().cursor.point = Point::new(Line(0), Column(0));
        let mut overlay = MathOverlay::default();
        assert!(!plan(&mut overlay, &term, 20, 8).is_empty());
        term.resize(TestSize { cols: 20, rows: 3 });
        let _ = plan(&mut overlay, &term, 20, 3);
    }
    #[test]
    fn image_projection_splits_an_older_snapshot_around_new_formula_cells() {
        use nebula_terminal::event_loop::StreamProcessor;
        use nebula_terminal::inline_image::ImageLayout;
        use nebula_terminal::render::{RenderSnapshot, SnapshotConfig};
        let mut term = Term::new(Config::default(), &TestSize { cols: 20, rows: 4 }, VoidListener);
        term.grid_mut().cursor.point = Point::new(Line(0), Column(2));
        let _placement = term.nebula_insert_inline_image(ImageLayout {
            width: 64.0,
            height: 18.0,
            cell_width: 8.0,
            cell_height: 18.0,
            columns: 8,
            rows: 1,
        });
        let old = RenderSnapshot::capture(&term, &SnapshotConfig { rows: 4, cols: 20 });
        assert_eq!((old.image_runs[0].column, old.image_runs[0].columns), (2, 8));
        let mut stream = StreamProcessor::default();
        stream.feed(&mut term, &VoidListener, b"\x1b[1;5H$x$\r\nprompt");
        let mut overlay = MathOverlay::default();
        let frame = plan(&mut overlay, &term, 20, 4);
        assert_eq!(frame.formulas.len(), 1);
        let projected = frame.project_image_runs(&old.image_runs, 20);
        assert_eq!(projected.len(), 2);
        assert_eq!(
            (projected[0].column, projected[0].source_column, projected[0].columns),
            (2, 0, 2)
        );
        let second_column = frame.project_cell(0, 7, 20).unwrap() as u16;
        assert_eq!(
            (projected[1].column, projected[1].source_column, projected[1].columns),
            (second_column, 5, 3)
        );
    }
}
