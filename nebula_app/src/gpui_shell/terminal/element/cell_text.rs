//! 单格文本的塑形与落笔（网格主路径、ghost 与补全弹窗共用）。
//!
//! 定位合同与 [`super`] 模块头一致：字形落在 `列号 × cell_width`，不使用
//! `force_width` 的 1px 吸附容差；每个可见字符单独塑形、从整数 cell 原点
//! 起笔，ASCII 连字由 `ligatures` 显式映射字形簇到列号。这里再补一条
//! 跨度合同：字形不得把墨迹压进右邻格——自然 advance 超过自己格子跨度
//! 的（彩色 emoji、East Asian Ambiguous 的全宽 fallback）缩进跨度再画；
//! 彩色 emoji 的 advance 被 macOS 位图 strike 量化，只能迭代收敛（见
//! [`fit_emoji_font_size`]）。

use gpui::{App, Pixels, SharedString, TextRun, Window, point};

use super::Hsla;

/// Private Use Area（Nerd Font 图标、powerline 分隔符）：刻意画满甚至越出
/// 格子拼接徽章，缩了提示符图标就整体变小（旧壳 2026-07-28 用户反馈裁定，
/// 见 `renderer/text/glyph_cache.rs` 同款判据）。
fn is_private_use(character: char) -> bool {
    matches!(u32::from(character), 0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD)
}

/// 超宽字形的缩放比；`None` = 保持原字号。彩色 emoji 的自然 advance
/// （Segoe UI Emoji ≈ 1.37em）常超过自己的网格槽（宽字符两列：0.6em 步进的
/// 主字体 ≈ 1.2em），而逐格定位合同下右邻字形永远从下一格起笔，溢出的
/// 墨迹就压在邻格上（用户 10-10 报：agent 界面里的 emoji 与右侧内容
/// 紧贴）。宽字符的正常全宽字形 advance 恰等于跨度（CJK 主路径 1.2em），
/// 容差收到 1.1 只捕获 emoji；窄格维持旧壳 1.4 容差（放过斜体悬伸，捕获
/// ① 这类落到全宽 fallback 的 East Asian Ambiguous 字）。
fn overwide_shrink_ratio(text: &str, natural: Pixels, span: Pixels, wide: bool) -> Option<f32> {
    if natural <= span || text.chars().next().is_some_and(is_private_use) {
        return None;
    }
    let tolerance = if wide { 1.1 } else { 1.4 };
    (natural > span * tolerance).then(|| f32::from(span) / f32::from(natural))
}

/// 彩色 emoji 迭代收敛的步数上限。台阶 advance 一步一档，实测 2 步内收敛；
/// 上限只兜数值异常，正常路径先被「已入槽」或「无进展」挡下。
const EMOJI_FIT_MAX_STEPS: usize = 4;

/// emoji 收敛目标在跨度右端预留的空隙比例（占跨度 1/6 = 宽槽 0.2em）。
/// Windows 的 Segoe UI Emoji 墨迹右轴承 ≈ 0.2em，advance 缩到跨度后天然
/// 留白；macOS 的 AppleColorEmoji 墨迹几乎铺满 advance，收到跨度后右缘
/// 仍贴着右邻格（用户 10-12 复验：间距有改观但仍偏小）。把目标收到跨度
/// 的 5/6（宽槽 1.2em → 1.0em advance，空隙 ≈ 0.2em ≈ 1/3 格），对齐
/// Windows 已验收的观感——emoji 比名义字号小约 14% 属两个平台共同的
/// 既定取舍（2026-10-10 note 的 Consequences）。
const EMOJI_FIT_AIR_SPAN_RATIO: f32 = 1.0 / 6.0;

/// 迭代收敛的目标宽度：跨度右端预留固定比例空隙后的值。
fn emoji_fit_target(span: f32) -> f32 {
    span * (1.0 - EMOJI_FIT_AIR_SPAN_RATIO)
}

/// 把字号迭代缩到「测得 advance ≤ 目标宽度」，返回最终字号。
///
/// macOS 的彩色 emoji 走 AppleColorEmoji 位图回退，advance 被位图 strike
/// 量化成绝对像素台阶（2026-10-12 本机实测：15px 字号 → 20px advance、
/// 13.5px → 18px、13px → 17px；10–19px 字号区普遍超出两列槽 1.2em 达
/// 0.2–1.8px），既不随字号线性缩放，也常落在 [`overwide_shrink_ratio`]
/// 1.1 容差带内（实测比值 1.009–1.14）——一次性按 `跨度/自然宽` 缩字号
/// 对线性 advance（Windows Segoe UI Emoji ≈ 1.373em）一步命中，对量化
/// 台阶则会差一档（14px 档缩一次后 18px 仍溢出 16.8px 槽 1.2px）。所以
/// emoji 字形改为以目标宽度（[`emoji_fit_target`]，跨度减空隙）为终点的
/// 迭代：缩一档、再量一次，直到入槽、无进展（advance 不再随字号下降）
/// 或步数上限。
fn fit_emoji_font_size(target: f32, font_size: f32, mut measure: impl FnMut(f32) -> f32) -> f32 {
    let mut size = font_size;
    let mut width = measure(size);
    for _ in 0..EMOJI_FIT_MAX_STEPS {
        if width <= target {
            break;
        }
        let next = size * (target / width);
        if next >= size {
            break;
        }
        size = next;
        let measured = measure(size);
        if measured >= width {
            break;
        }
        width = measured;
    }
    size
}

/// 单格文本塑形（含超宽缩进）：溢出跨度的字形按 `跨度 / 自然宽` 缩小
/// 字号重新塑形——CPU 栅格化进文本系统缓存，不做 GPU 拉伸，与旧壳 GL
/// 渲染器在 `glyph_cache::load_glyph` 的装载期位图缩放（2026-07-28 ①
/// 叠字修复）同一条取舍。缩小后的行高由 `ShapedLine::paint` 按
/// `line_height` 垂直居中，不会贴到上下行。
///
/// 彩色 emoji 回退字形例外：macOS 位图 strike 量化 advance（见
/// [`fit_emoji_font_size`]），走迭代收敛而非一次性比例缩放。判定信号是
/// gpui 塑形结果里的 `is_emoji` 字形标志（macOS CoreText 对
/// AppleColorEmoji 置位；Windows DirectWrite 不置位，维持既有线性路径，
/// 已验证的 Windows 行为不变）。
pub(super) fn shape_cell_text(
    window: &mut Window,
    text: &str,
    font_size: Pixels,
    run: &TextRun,
    span: Pixels,
    wide: bool,
) -> gpui::ShapedLine {
    let shared: SharedString = text.into();
    let runs = std::slice::from_ref(run);
    let shaped = window.text_system().shape_line(shared.clone(), font_size, runs, None);
    if shaped.runs.iter().any(|run| run.glyphs.iter().any(|glyph| glyph.is_emoji)) {
        let size =
            fit_emoji_font_size(emoji_fit_target(f32::from(span)), f32::from(font_size), |size| {
                f32::from(
                    window
                        .text_system()
                        .shape_line(shared.clone(), gpui::px(size), runs, None)
                        .width,
                )
            });
        // 末次塑形命中行缓存：迭代里已量过同一 (文本, 字号)，这里零成本取回。
        return window.text_system().shape_line(shared, gpui::px(size), runs, None);
    }
    match overwide_shrink_ratio(text, shaped.width, span, wide) {
        Some(ratio) => window.text_system().shape_line(
            shared,
            gpui::px(f32::from(font_size) * ratio),
            runs,
            None,
        ),
        None => shaped,
    }
}

/// 单格文本落笔原语：单 cell 文本塑形后从调用方给定的整数 cell
/// 原点起笔，不传 `force_width`（首字形天然在 x=0，位置只由列号决定）。
fn paint_cell_text(
    window: &mut Window,
    cx: &mut App,
    text: SharedString,
    run: TextRun,
    font_size: Pixels,
    origin: gpui::Point<Pixels>,
    line_height: Pixels,
    span: Pixels,
    wide: bool,
) {
    let shaped = shape_cell_text(window, &text, font_size, &run, span, wide);
    let _ = shaped.paint(origin, line_height, gpui::TextAlign::Left, None, window, cx);
}

/// 网格对齐的浮层文本（ghost/弹窗共用），与网格文字同一定位合同：每个可见
/// 字符单独塑形、从 `起始格 + 已用格数` 的整数 cell 原点起笔，超出
/// `max_cells` 截断。绝不批量 `force_width`——其 1px 容差会沿字符累积并在
/// 重塑形时翻转吸附，删除 `bypassPermission` 一类提示时就会逐字向左挤。
/// 单字符行命中 GPUI 行缓存，逐字塑形没有额外热路径成本。
#[allow(clippy::too_many_arguments)]
pub(super) fn grid_text(
    window: &mut Window,
    cx: &mut App,
    text: &str,
    font: &gpui::Font,
    font_size: Pixels,
    color: Hsla,
    origin: gpui::Point<Pixels>,
    cell_width: Pixels,
    line_height: Pixels,
    max_cells: usize,
) -> usize {
    use unicode_width::UnicodeWidthChar as _;

    let mut used = 0usize;
    for character in text.chars() {
        // 与旧壳 draw_string 一致：零宽字符不单占格，宽字符占两格。
        let width = character.width().unwrap_or(0);
        if width == 0 {
            continue;
        }
        if used + width > max_cells {
            break;
        }
        let text = character.to_string();
        let run = TextRun {
            len: text.len(),
            font: font.clone(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        paint_cell_text(
            window,
            cx,
            SharedString::from(text),
            run,
            font_size,
            point(origin.x + cell_width * used as f32, origin.y),
            line_height,
            cell_width * width as f32,
            width == 2,
        );
        used += width;
    }
    used
}

#[cfg(test)]
mod tests {
    /// emoji/全宽 fallback 的缩放判据：只缩真正溢出跨度的字形，PUA 图标与
    /// 容差内的正常越界保持原样（口径同旧壳 `glyph_cache::load_glyph`）。
    mod overwide_fitting {
        use super::super::overwide_shrink_ratio;
        use gpui::px;

        #[test]
        fn wide_emoji_overflow_shrinks_to_its_two_column_span() {
            // 实测口径：Segoe UI Emoji advance ≈ 1.37em，主字体 Maple Mono NF
            // CN 步进 0.6em，两列槽 = 1.2em（CJK 恰好铺满）。
            let span = px(12.0);
            let natural = px(13.7);
            assert_eq!(overwide_shrink_ratio("👍", natural, span, true), Some(12.0 / 13.7));
            // 正常全宽字形 advance 恰等于跨度（CJK 主路径），不缩。
            assert_eq!(overwide_shrink_ratio("中", span, span, true), None);
            // 容差内的轻微越界（紧凑 cell 模式取整）不缩，避免全部 CJK 被重栅格化。
            assert_eq!(overwide_shrink_ratio("中", px(12.4), span, true), None);
            assert_eq!(overwide_shrink_ratio("中", px(13.3), span, true), Some(12.0 / 13.3));
        }

        #[test]
        fn narrow_fallback_keeps_the_legacy_tolerance() {
            // ① 落到全宽 fallback：1 列槽、≈2 列墨迹，超过 1.4 容差才缩（旧壳
            // 2026-07-28 ① 叠字修复的同一组数）。
            let span = px(6.0);
            assert_eq!(overwide_shrink_ratio("①", px(12.0), span, false), Some(0.5));
            assert_eq!(overwide_shrink_ratio("W", px(7.5), span, false), None);
            assert_eq!(overwide_shrink_ratio("W", px(8.4), span, false), None);
            assert_eq!(overwide_shrink_ratio("W", px(8.5), span, false), Some(6.0 / 8.5));
        }

        #[test]
        fn nerd_font_icons_keep_their_ink_even_when_overwide() {
            // PUA 图标画满格子拼徽章是刻意的，缩了提示符整体变小（旧壳裁定）。
            assert_eq!(overwide_shrink_ratio("\u{f0ce5}", px(13.7), px(12.0), true), None);
            assert_eq!(overwide_shrink_ratio("\u{e0b0}", px(12.0), px(6.0), false), None);
        }
    }

    /// 彩色 emoji 回退字形的迭代收敛（macOS 位图 strike 量化 advance，
    /// 见 [`super::super::fit_emoji_font_size`] 的取证注释）。
    mod emoji_strike_fitting {
        use super::super::{emoji_fit_target, fit_emoji_font_size};

        #[test]
        fn quantized_advance_converges_by_iteration() {
            // macOS 实测口径（15px 字号）：advance 20px > 两列槽 18px，缩到
            // 13.5px 后 advance 落到 18px。一步比例缩放碰巧能命中这一档，
            // 但 14px 档（19 → 16.8 槽）一步只能到 18px，仍溢出——迭代把
            // 「再量一次」变成算法的一部分。
            let size =
                fit_emoji_font_size(18.0, 15.0, |size| if size >= 14.0 { 20.0 } else { 18.0 });
            assert!((size - 13.5).abs() < 1e-6, "一步后 advance 18 恰等于槽，收敛于 13.5px");
            let size =
                fit_emoji_font_size(16.8, 14.0, |size| if size >= 13.0 { 19.0 } else { 17.0 });
            assert!(size < 13.0, "19 与 17 都超 16.8 槽，须缩过 13px 台阶");
        }

        #[test]
        fn strike_floor_stops_when_advance_no_longer_shrinks() {
            // 台阶下限：字号继续缩小、advance 不再下降时必须退出，不能震荡。
            let mut calls = 0;
            let size = fit_emoji_font_size(16.8, 14.0, |_| {
                calls += 1;
                17.0
            });
            assert!(calls <= 2, "首测 + 一次无进展退出，测量次数 {calls}");
            assert!(size < 14.0, "无进展前的最后一次有效缩小被保留");
        }

        #[test]
        fn linear_advance_converges_quickly() {
            // Windows Segoe UI Emoji 一类线性 advance：一两步内贴住槽。
            let size = fit_emoji_font_size(18.0, 15.0, |size| size * 1.373);
            assert!((size * 1.373 - 18.0).abs() <= 0.05, "线性字形最终 advance 贴合槽");
        }

        #[test]
        fn within_span_keeps_the_requested_size() {
            // 大字号档（advance ≤ 槽）不付任何重塑形。
            assert!((fit_emoji_font_size(24.0, 15.0, |size| size * 1.2) - 15.0).abs() < 1e-6);
        }

        #[test]
        fn fit_target_reserves_a_sixth_of_the_span_as_trailing_air() {
            // 宽槽（默认字号 15px：2 × 9px = 18px）→ 目标 15px = 1.0em
            // advance，右缘空隙 ≈ 0.2em，对齐 Windows Segoe 右轴承的观感。
            assert!((emoji_fit_target(18.0) - 15.0).abs() < 1e-6);
            // 窄槽同比例：9px → 7.5px。
            assert!((emoji_fit_target(9.0) - 7.5).abs() < 1e-6);
        }

        // macOS 端到端无法进 cargo test：真字体塑形需要构造 Application，
        // 而 macOS 上 App 必须在主线程（test harness 的测试线程不满足）。
        // 端到端链的取证见 note
        // 2026-10-12-macos-emoji-strike-quantization.md：gpui_macos 对
        // AppleColorEmoji 置 is_emoji（源码）+ CoreText 级联实测（run 落到
        // AppleColorEmoji、advance 台阶表）+ 发布构建人工目检。
    }
}
