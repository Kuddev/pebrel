//! 提示符 ssh 标签里那枚图标：把项目自带的 Claude 品牌图（侧栏标签页标题前
//! 同一张 `extra/logo/ai_claude.png`）准备成可以直接画在终端格子上的贴图。
//!
//! 网格里只有字符，位图进不了 PS1，所以分工是：`nebula_terminal` 的提示符往
//! 那一格写回落字形（别的终端与旧壳看到的就是它），宿主绘制时用这张品牌图盖
//! 在格子上。纹理全进程准备一次；每帧只多一次纹理采样，不重新解码。

use std::sync::{Arc, OnceLock};

use gpui::{Bounds, Corners, Pixels, Point, RenderImage, Window, point, px, size};
use image::Frame;

/// 品牌图预缩放到的边长（物理像素）：图标画在十几个逻辑像素的格子里，96 足够
/// 覆盖 200% 缩放下的取样，同时避开绘制期缩放 1024px 原图产生的灰边。
const TARGET_PX: u32 = 96;

static CLAUDE_MARK: OnceLock<Option<Arc<RenderImage>>> = OnceLock::new();

/// Claude 品牌图。读不到时返回 `None`：绘制路径保住网格里那枚回落字形，
/// 不做第二份替代图形。
pub(super) fn claude_mark() -> Option<Arc<RenderImage>> {
    CLAUDE_MARK
        .get_or_init(|| {
            let rgba = image::load_from_memory(crate::display::AiLogo::Claude.png(false))
                .ok()?
                .into_rgba8();
            prepare(rgba)
        })
        .clone()
}

/// 终端网格的几何：图标格与字形走同一套"列号 × 列宽"定位合同。
#[derive(Clone, Copy)]
pub(super) struct ChipGrid {
    pub origin: Point<Pixels>,
    pub cell_width: Pixels,
    pub line_height: Pixels,
}

/// 把提示符 ssh 标签的图标格画成品牌图。字形那一格由调用方在文本层跳过
/// （见 `element.rs` 的逐格绘制），这里只画位图；`clip` 是终端区域的裁剪框。
pub(super) fn paint(
    window: &mut Window,
    clip: Bounds<Pixels>,
    grid: ChipGrid,
    icons: &[(u16, u16)],
    visual_column: impl Fn(u16, u16) -> usize,
) {
    let Some(image) = claude_mark() else { return };
    // 方图略大于一格：两侧的溢出落在标签自带的空格上（`" <icon> ssh "`），
    // 换来品牌图在行高里能看清的那点尺寸。
    let side = (grid.line_height.as_f32() * 0.9).min(grid.cell_width.as_f32() * 1.6).max(1.0);
    for &(row, col) in icons {
        let left = grid.origin.x + grid.cell_width * visual_column(row, col) as f32;
        let top = grid.origin.y + grid.line_height * f32::from(row);
        let icon = Bounds::new(
            point(
                left + (grid.cell_width - px(side)) / 2.0,
                top + (grid.line_height - px(side)) / 2.0,
            ),
            size(px(side), px(side)),
        );
        let _ = window.paint_image(clip, icon, Corners::all(px(0.0)), image.clone(), 0, false);
    }
}

fn prepare(rgba: image::RgbaImage) -> Option<Arc<RenderImage>> {
    // 与侧栏那套共用预处理：Lanczos3 预缩放 + alpha 质心校正，保证小尺寸下
    // 墨迹居中、不带原图 1024px 的灰边。
    let (prepared, width, height) = crate::display::prepare_ai_logo_texture(
        rgba.as_raw(),
        rgba.width(),
        rgba.height(),
        TARGET_PX,
    );
    let mut rgba = image::RgbaImage::from_raw(width, height, prepared)?;
    // GPUI 的原始帧走 BGRA，与侧栏贴图和终端图片解码同一套通道转换。
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Some(Arc::new(RenderImage::new([Frame::new(rgba)])))
}
