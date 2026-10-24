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
    pub blink: super::cursor_blink::BlinkFrame,
    pub block_color: Rgba,
    pub stroke: Rgba,
    pub text_color: Option<Hsla>,
}

impl CursorPaint {
    pub(super) fn visual_rect(&self) -> Bounds<Pixels> {
        let mut rect = self.rect;
        if self.shape == CursorShape::Beam {
            rect.size.width = px(2.0);
        }
        if self.shape == CursorShape::Underline {
            rect.origin.y = rect.bottom() - px(2.0);
            rect.size.height = px(2.0);
        }
        let factor = if self.focused { self.blink.height } else { 1.0 };
        rect.origin.y += rect.size.height * ((1.0 - factor) * 0.5);
        rect.size.height *= factor;
        rect
    }

    pub(super) fn visual_color(&self, color: Rgba) -> Rgba {
        Rgba { a: color.a * if self.focused { self.blink.opacity } else { 1.0 }, ..color }
    }

    pub fn paint_background(&self, window: &mut Window) {
        if self.focused && self.visible && self.shape == CursorShape::Block {
            window.paint_quad(fill(self.visual_rect(), self.visual_color(self.block_color)));
        }
    }

    pub fn paint_foreground(&self, window: &mut Window) {
        if self.focused && !self.visible {
            return;
        }
        match (self.focused, self.shape) {
            (false, CursorShape::Block | CursorShape::HollowBlock)
            | (true, CursorShape::HollowBlock) => {
                window.paint_quad(outline(
                    self.visual_rect(),
                    self.visual_color(self.stroke),
                    gpui::BorderStyle::Solid,
                ));
            },
            (_, CursorShape::Beam | CursorShape::Underline) => {
                window.paint_quad(fill(self.visual_rect(), self.visual_color(self.stroke)));
            },
            _ => {},
        }
    }

    pub fn inverted_color(&self, bounds: Bounds<Pixels>) -> Option<Hsla> {
        (self.focused
            && self.visible
            && self.shape == CursorShape::Block
            && self.visual_rect().intersects(&bounds))
        .then_some(self.text_color)
        .flatten()
        .map(|mut color| {
            color.a *= self.blink.opacity;
            color
        })
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
        window.with_content_mask(Some(ContentMask { bounds: self.visual_rect() }), |window| {
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expand_keeps_every_shape_centered_and_fades_include_inverted_text() {
        let mut cursor = CursorPaint {
            rect: Bounds::new(point(px(10.0), px(20.0)), size(px(9.0), px(18.0))),
            shape: CursorShape::Block,
            focused: true,
            visible: true,
            blink: super::super::cursor_blink::BlinkFrame { opacity: 0.4, height: 0.5 },
            block_color: gpui::white().into(),
            stroke: gpui::white().into(),
            text_color: Some(gpui::black()),
        };
        for shape in [
            CursorShape::Block,
            CursorShape::Beam,
            CursorShape::Underline,
            CursorShape::HollowBlock,
        ] {
            cursor.shape = shape;
            let visual = cursor.visual_rect();
            assert_eq!(
                visual.center().y,
                if shape == CursorShape::Underline { px(37.0) } else { px(29.0) }
            );
            assert_eq!(
                visual.size.height,
                if shape == CursorShape::Underline { px(1.0) } else { px(9.0) }
            );
            assert_eq!(cursor.visual_color(cursor.stroke).a, 0.4);
        }
        cursor.shape = CursorShape::Block;
        assert_eq!(cursor.inverted_color(cursor.rect).unwrap().a, 0.4);
        cursor.focused = false;
        assert_eq!(cursor.visual_rect(), cursor.rect);
        assert_eq!(cursor.visual_color(cursor.stroke).a, 1.0);
    }
}
