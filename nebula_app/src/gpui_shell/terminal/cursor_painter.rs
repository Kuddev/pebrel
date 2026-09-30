//! A shared visual rectangle for all host cursor shapes and clipped inversion.

use gpui::{
    App, Bounds, ContentMask, Hsla, Pixels, Point, Rgba, ShapedLine, TextRun, Window, fill,
    outline, point, px, size,
};
use nebula_terminal::vte::ansi::CursorShape;

pub(super) struct CursorPaint {
    pub rect: Bounds<Pixels>,
    pub shape: CursorShape,
    pub focused: bool,
    pub visible: bool,
    pub block_color: Rgba,
    pub stroke: Rgba,
    pub text_color: Option<Hsla>,
}

impl CursorPaint {
    pub fn paint_background(&self, window: &mut Window) {
        if self.focused && self.visible && self.shape == CursorShape::Block {
            window.paint_quad(fill(self.rect, self.block_color));
        }
    }

    pub fn paint_foreground(&self, window: &mut Window) {
        if self.focused && !self.visible {
            return;
        }
        match (self.focused, self.shape) {
            (false, CursorShape::Block | CursorShape::HollowBlock)
            | (true, CursorShape::HollowBlock) => {
                window.paint_quad(outline(self.rect, self.stroke, gpui::BorderStyle::Solid));
            },
            (_, CursorShape::Beam) => {
                window.paint_quad(fill(
                    Bounds::new(self.rect.origin, size(px(2.0), self.rect.size.height)),
                    self.stroke,
                ));
            },
            (_, CursorShape::Underline) => {
                window.paint_quad(fill(
                    Bounds::new(
                        point(self.rect.origin.x, self.rect.bottom() - px(2.0)),
                        size(self.rect.size.width, px(2.0)),
                    ),
                    self.stroke,
                ));
            },
            _ => {},
        }
    }

    pub fn inverted_color(&self, bounds: Bounds<Pixels>) -> Option<Hsla> {
        (self.focused
            && self.visible
            && self.shape == CursorShape::Block
            && self.rect.intersects(&bounds))
        .then_some(self.text_color)
        .flatten()
    }

    /// Reuse the exact shaped layout, including ligature cluster positions.
    /// Only decoration colors differ, and the overlay is clipped to the cursor.
    pub fn paint_inverted_text(
        &self,
        shaped: &ShapedLine,
        mut run: TextRun,
        origin: Point<Pixels>,
        line_height: Pixels,
        window: &mut Window,
        cx: &mut App,
    ) {
        let bounds = Bounds::new(origin, size(shaped.width.max(px(1.0)), line_height));
        let Some(color) = self.inverted_color(bounds) else {
            return;
        };
        run.len = shaped.text.len();
        run.color = color;
        if let Some(underline) = &mut run.underline {
            underline.color = Some(color);
        }
        if let Some(strike) = &mut run.strikethrough {
            strike.color = Some(color);
        }
        let mut inverted =
            window.text_system().shape_line(shaped.text.clone(), shaped.font_size, &[run], None);
        *inverted = std::ops::Deref::deref(shaped).clone();
        window.with_content_mask(Some(ContentMask { bounds: self.rect }), |window| {
            let _ = inverted.paint(origin, line_height, gpui::TextAlign::Left, None, window, cx);
        });
    }
}

pub(super) fn paint_box_glyph(
    window: &mut Window,
    prims: &[nebula_terminal::render::boxdraw::Primitive],
    origin: Point<Pixels>,
    color: Rgba,
) {
    use nebula_terminal::render::boxdraw::Primitive;
    let at = |p: &[f32; 2]| point(origin.x + px(p[0]), origin.y + px(p[1]));
    for prim in prims {
        match prim {
            Primitive::Rect { rect, alpha } => window.paint_quad(fill(
                Bounds::new(
                    point(origin.x + px(rect.x), origin.y + px(rect.y)),
                    size(px(rect.w), px(rect.h)),
                ),
                Rgba { a: color.a * alpha, ..color },
            )),
            Primitive::Poly { points } => {
                let Some((first, rest)) = points.split_first() else { continue };
                let mut path = gpui::Path::new(at(first));
                for p in rest {
                    path.line_to(at(p));
                }
                window.paint_path(path, color);
            },
        }
    }
}
